use crate::{
    FormatPromptArgs, PredictArgs, PredictionProvider, TeacherBackend,
    anthropic_client::AnthropicClient,
    example::{Example, ExamplePrediction},
    format_prompt::{TeacherMultiRegionPrompt, TeacherPrompt, run_format_prompt},
    headless::EpAppState,
    openai_client::OpenAiClient,
    parse_output::parse_prediction_output,
    progress::{ExampleProgress, Progress, Step, StepProgress},
    retrieve_context::run_context_retrieval,
};
use anyhow::Context as _;
use cloud_llm_client::predict_edits_v3::{RawCompletionRequest, RawCompletionResponse};
use futures::AsyncReadExt as _;
use gpui::AsyncApp;
use http_client::{AsyncBody, HttpClient, Method};
use reqwest_client::ReqwestClient;
use std::sync::{
        Arc, OnceLock,
    };
use zeta_prompt::ZetaFormat;

static ANTHROPIC_CLIENT: OnceLock<AnthropicClient> = OnceLock::new();
static OPENAI_CLIENT: OnceLock<OpenAiClient> = OnceLock::new();

pub async fn run_prediction(
    example: &mut Example,
    args: &PredictArgs,
    app_state: Arc<EpAppState>,
    example_progress: &ExampleProgress,
    cx: AsyncApp,
) -> anyhow::Result<()> {
    let repetition_count = args.repetitions;

    if let Some(existing_prediction) = example.predictions.first() {
        let has_prediction = existing_prediction.actual_patch.is_some()
            || !existing_prediction.actual_output.is_empty();
        if has_prediction {
            match args.provider {
                None => return Ok(()),
                Some(provider) if existing_prediction.provider == provider => return Ok(()),
                Some(_) => example.predictions.clear(),
            }
        }
    }

    let Some(provider) = args.provider else {
        anyhow::bail!(
            "No existing predictions found. Use --provider to specify which model to use for prediction."
        );
    };

    if matches!(
        provider,
        PredictionProvider::TeacherMultiRegion(..)
            | PredictionProvider::TeacherMultiRegionNonBatching(..)
    ) {
        anyhow::bail!("Teacher multi-region providers are not supported for prediction.");
    }

    if let PredictionProvider::Teacher(backend, _)
    | PredictionProvider::TeacherNonBatching(backend, _) = provider
    {
        run_context_retrieval(example, app_state.clone(), example_progress, cx.clone()).await?;
        run_format_prompt(
            example,
            &FormatPromptArgs { provider },
            app_state.clone(),
            example_progress,
            cx,
        )
        .await?;

        let step_progress = example_progress.start(Step::Predict);
        let batched = matches!(
            provider,
            PredictionProvider::Teacher(..) | PredictionProvider::TeacherMultiRegion(..)
        );
        return predict_teacher(
            example,
            backend,
            batched,
            repetition_count,
            args.cache_only,
            &step_progress,
        )
        .await;
    }

    if let PredictionProvider::Baseten(format) = provider {
        run_format_prompt(
            example,
            &FormatPromptArgs {
                provider: PredictionProvider::Zeta2(format),
            },
            app_state.clone(),
            example_progress,
            cx,
        )
        .await?;

        let step_progress = example_progress.start(Step::Predict);
        return predict_baseten(example, format, &step_progress).await;
    }

    let _ = (app_state, cx);
    anyhow::bail!(
        "The Zed-hosted edit prediction providers (Zeta and Mercury) have been removed. \
         No CLI-supported in-process providers remain."
    );
}

async fn predict_teacher(
    example: &mut Example,
    backend: TeacherBackend,
    batched: bool,
    repetition_count: usize,
    cache_only: bool,
    step_progress: &crate::progress::StepProgress,
) -> anyhow::Result<()> {
    match backend {
        TeacherBackend::Sonnet45 | TeacherBackend::Sonnet46 => {
            predict_anthropic(
                example,
                backend,
                batched,
                repetition_count,
                cache_only,
                step_progress,
            )
            .await
        }
        TeacherBackend::Gpt52 => {
            predict_openai(
                example,
                backend,
                batched,
                repetition_count,
                cache_only,
                step_progress,
            )
            .await
        }
    }
}

async fn predict_anthropic(
    example: &mut Example,
    backend: TeacherBackend,
    batched: bool,
    repetition_count: usize,
    cache_only: bool,
    step_progress: &crate::progress::StepProgress,
) -> anyhow::Result<()> {
    let llm_model_name = backend.model_name();
    let max_tokens = 16384;
    let llm_client = ANTHROPIC_CLIENT.get_or_init(|| {
        let client = if batched {
            AnthropicClient::batch(&crate::paths::LLM_CACHE_DB)
        } else {
            AnthropicClient::plain()
        };
        client.expect("Failed to create Anthropic client")
    });

    let prompt = example.prompt.as_ref().context("Prompt is required")?;

    for ix in 0..repetition_count {
        if repetition_count > 1 {
            step_progress.set_substatus(format!(
                "running prediction {}/{}",
                ix + 1,
                repetition_count
            ));
        } else {
            step_progress.set_substatus("running prediction");
        }

        let messages = vec![anthropic::Message {
            role: anthropic::Role::User,
            content: vec![anthropic::RequestContent::Text {
                text: prompt.input.clone(),
                cache_control: None,
            }],
        }];

        let seed = if repetition_count > 1 { Some(ix) } else { None };
        let Some(response) = llm_client
            .generate(llm_model_name, max_tokens, messages, seed, cache_only)
            .await?
        else {
            // Request stashed for batched processing
            continue;
        };

        let actual_output = response
            .content
            .into_iter()
            .filter_map(|content| match content {
                anthropic::ResponseContent::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join("\n");

        let parser_provider = if batched {
            example
                .prompt
                .as_ref()
                .map(|prompt| prompt.provider)
                .unwrap_or(PredictionProvider::Teacher(backend, ZetaFormat::default()))
        } else {
            match example.prompt.as_ref().map(|prompt| prompt.provider) {
                Some(PredictionProvider::TeacherMultiRegion(_))
                | Some(PredictionProvider::TeacherMultiRegionNonBatching(_)) => {
                    PredictionProvider::TeacherMultiRegionNonBatching(backend)
                }
                _ => PredictionProvider::TeacherNonBatching(backend, ZetaFormat::default()),
            }
        };

        let (actual_patch, actual_cursor) = match parser_provider {
            PredictionProvider::TeacherMultiRegion(_)
            | PredictionProvider::TeacherMultiRegionNonBatching(_) => {
                TeacherMultiRegionPrompt::parse(example, &actual_output)?
            }
            _ => TeacherPrompt::parse(example, &actual_output)?,
        };

        let prediction = ExamplePrediction {
            actual_patch: Some(actual_patch),
            actual_output,
            actual_cursor,
            error: None,
            provider: if batched {
                match example.prompt.as_ref().map(|prompt| prompt.provider) {
                    Some(PredictionProvider::TeacherMultiRegion(_)) => {
                        PredictionProvider::TeacherMultiRegion(backend)
                    }
                    _ => PredictionProvider::Teacher(backend, ZetaFormat::default()),
                }
            } else {
                match example.prompt.as_ref().map(|prompt| prompt.provider) {
                    Some(PredictionProvider::TeacherMultiRegion(_))
                    | Some(PredictionProvider::TeacherMultiRegionNonBatching(_)) => {
                        PredictionProvider::TeacherMultiRegionNonBatching(backend)
                    }
                    _ => PredictionProvider::TeacherNonBatching(backend, ZetaFormat::default()),
                }
            },
            cumulative_logprob: None,
            avg_logprob: None,
        };

        example.predictions.push(prediction);
    }
    Ok(())
}

async fn predict_openai(
    example: &mut Example,
    backend: TeacherBackend,
    batched: bool,
    repetition_count: usize,
    cache_only: bool,
    step_progress: &crate::progress::StepProgress,
) -> anyhow::Result<()> {
    let llm_model_name = backend.model_name();
    let max_tokens = 16384;
    let llm_client = OPENAI_CLIENT.get_or_init(|| {
        let client = if batched {
            OpenAiClient::batch(&crate::paths::LLM_CACHE_DB)
        } else {
            OpenAiClient::plain()
        };
        client.expect("Failed to create OpenAI client")
    });

    let prompt = example.prompt.as_ref().context("Prompt is required")?;

    for ix in 0..repetition_count {
        if repetition_count > 1 {
            step_progress.set_substatus(format!(
                "running prediction {}/{}",
                ix + 1,
                repetition_count
            ));
        } else {
            step_progress.set_substatus("running prediction");
        }

        let messages = vec![open_ai::RequestMessage::User {
            content: open_ai::MessageContent::Plain(prompt.input.clone()),
        }];

        let seed = if repetition_count > 1 { Some(ix) } else { None };
        let Some(response) = llm_client
            .generate(llm_model_name, max_tokens, messages, seed, cache_only)
            .await?
        else {
            // Request stashed for batched processing
            continue;
        };

        let actual_output = response
            .choices
            .into_iter()
            .filter_map(|choice| match choice.message {
                open_ai::RequestMessage::Assistant { content, .. } => content.map(|c| match c {
                    open_ai::MessageContent::Plain(text) => text,
                    open_ai::MessageContent::Multipart(parts) => parts
                        .into_iter()
                        .filter_map(|p| match p {
                            open_ai::MessagePart::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(""),
                }),
                _ => None,
            })
            .collect::<Vec<String>>()
            .join("\n");

        let parser_provider = if batched {
            example
                .prompt
                .as_ref()
                .map(|prompt| prompt.provider)
                .unwrap_or(PredictionProvider::Teacher(backend, ZetaFormat::default()))
        } else {
            match example.prompt.as_ref().map(|prompt| prompt.provider) {
                Some(PredictionProvider::TeacherMultiRegion(_))
                | Some(PredictionProvider::TeacherMultiRegionNonBatching(_)) => {
                    PredictionProvider::TeacherMultiRegionNonBatching(backend)
                }
                _ => PredictionProvider::TeacherNonBatching(backend, ZetaFormat::default()),
            }
        };

        let (actual_patch, actual_cursor) = match parser_provider {
            PredictionProvider::TeacherMultiRegion(_)
            | PredictionProvider::TeacherMultiRegionNonBatching(_) => {
                TeacherMultiRegionPrompt::parse(example, &actual_output)?
            }
            _ => TeacherPrompt::parse(example, &actual_output)?,
        };

        let prediction = ExamplePrediction {
            actual_patch: Some(actual_patch),
            actual_output,
            actual_cursor,
            error: None,
            provider: if batched {
                match example.prompt.as_ref().map(|prompt| prompt.provider) {
                    Some(PredictionProvider::TeacherMultiRegion(_)) => {
                        PredictionProvider::TeacherMultiRegion(backend)
                    }
                    _ => PredictionProvider::Teacher(backend, ZetaFormat::default()),
                }
            } else {
                match example.prompt.as_ref().map(|prompt| prompt.provider) {
                    Some(PredictionProvider::TeacherMultiRegion(_))
                    | Some(PredictionProvider::TeacherMultiRegionNonBatching(_)) => {
                        PredictionProvider::TeacherMultiRegionNonBatching(backend)
                    }
                    _ => PredictionProvider::TeacherNonBatching(backend, ZetaFormat::default()),
                }
            },
            cumulative_logprob: None,
            avg_logprob: None,
        };

        example.predictions.push(prediction);
    }
    Ok(())
}

pub async fn predict_baseten(
    example: &mut Example,
    format: ZetaFormat,
    step_progress: &StepProgress,
) -> anyhow::Result<()> {
    let model_id =
        std::env::var("ZED_ZETA_MODEL").context("ZED_ZETA_MODEL environment variable required")?;

    let api_key =
        std::env::var("BASETEN_API_KEY").context("BASETEN_API_KEY environment variable not set")?;

    let prompt = example.prompt.as_ref().context("Prompt is required")?;
    let prompt_text = prompt.input.clone();
    let prefill = prompt.prefill.clone().unwrap_or_default();

    step_progress.set_substatus("running prediction via baseten");

    let environment: String = <&'static str>::from(&format).to_lowercase();
    let url = format!(
        "https://model-{model_id}.api.baseten.co/environments/{environment}/sync/v1/completions"
    );

    let request_body = RawCompletionRequest {
        model: model_id,
        prompt: prompt_text.clone(),
        max_tokens: Some(2048),
        temperature: Some(0.),
        stop: vec![],
        environment: None,
    };

    let body_bytes =
        serde_json::to_vec(&request_body).context("Failed to serialize request body")?;

    let http_client: Arc<dyn HttpClient> = Arc::new(ReqwestClient::new());
    let request = http_client::Request::builder()
        .method(Method::POST)
        .uri(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Api-Key {api_key}"))
        .body(AsyncBody::from(body_bytes))?;

    let mut response = http_client.send(request).await?;
    let status = response.status();

    let mut body = String::new();
    response
        .body_mut()
        .read_to_string(&mut body)
        .await
        .context("Failed to read Baseten response body")?;

    if !status.is_success() {
        anyhow::bail!("Baseten API returned {status}: {body}");
    }

    let completion: RawCompletionResponse =
        serde_json::from_str(&body).context("Failed to parse Baseten response")?;

    let actual_output = completion
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.text)
        .unwrap_or_default();

    let actual_output = format!("{prefill}{actual_output}");

    let (actual_patch, actual_cursor) =
        parse_prediction_output(example, &actual_output, PredictionProvider::Zeta2(format))?;

    let prediction = ExamplePrediction {
        actual_patch: Some(actual_patch),
        actual_output,
        actual_cursor,
        error: None,
        provider: PredictionProvider::Baseten(format),
        cumulative_logprob: None,
        avg_logprob: None,
    };

    example.predictions.push(prediction);
    Ok(())
}

pub async fn sync_batches(provider: Option<&PredictionProvider>) -> anyhow::Result<()> {
    match provider {
        Some(PredictionProvider::Teacher(backend, _))
        | Some(PredictionProvider::TeacherMultiRegion(backend)) => match backend {
            TeacherBackend::Sonnet45 | TeacherBackend::Sonnet46 => {
                let llm_client = ANTHROPIC_CLIENT.get_or_init(|| {
                    AnthropicClient::batch(&crate::paths::LLM_CACHE_DB)
                        .expect("Failed to create Anthropic client")
                });
                llm_client
                    .sync_batches()
                    .await
                    .context("Failed to sync Anthropic batches")?;
            }
            TeacherBackend::Gpt52 => {
                let llm_client = OPENAI_CLIENT.get_or_init(|| {
                    OpenAiClient::batch(&crate::paths::LLM_CACHE_DB)
                        .expect("Failed to create OpenAI client")
                });
                llm_client
                    .sync_batches()
                    .await
                    .context("Failed to sync OpenAI batches")?;
            }
        },
        _ => (),
    };
    Ok(())
}

pub async fn reprocess_after_batch_wait(
    examples: &mut [Example],
    args: &PredictArgs,
) -> anyhow::Result<()> {
    let Some(PredictionProvider::Teacher(backend, _)) = args.provider else {
        return Ok(());
    };

    let mut reprocessed = 0;
    for example in examples.iter_mut() {
        let has_prediction = example
            .predictions
            .iter()
            .any(|p| p.actual_patch.is_some() || !p.actual_output.is_empty());
        if has_prediction || example.prompt.is_none() {
            continue;
        }

        let example_progress = Progress::global().start_group(&example.spec.name);
        let step_progress = example_progress.start(Step::Predict);
        predict_teacher(
            example,
            backend,
            true,
            args.repetitions,
            false,
            &step_progress,
        )
        .await?;
        reprocessed += 1;
    }

    if reprocessed > 0 {
        eprintln!("Reprocessed {} example(s) with batch results", reprocessed);
    }

    Ok(())
}

pub async fn wait_for_batches(provider: Option<&PredictionProvider>) -> anyhow::Result<()> {
    let poll_interval = std::time::Duration::from_secs(30);

    loop {
        let pending = pending_batch_count(provider)?;
        if pending == 0 {
            break;
        }

        eprintln!(
            "Waiting for {} pending batch request(s) to complete... (polling every {}s)",
            pending,
            poll_interval.as_secs()
        );
        std::thread::sleep(poll_interval);

        sync_batches(provider).await?;
    }

    Ok(())
}

fn pending_batch_count(provider: Option<&PredictionProvider>) -> anyhow::Result<usize> {
    match provider {
        Some(PredictionProvider::Teacher(backend, _)) => match backend {
            TeacherBackend::Sonnet45 | TeacherBackend::Sonnet46 => {
                let llm_client = ANTHROPIC_CLIENT.get_or_init(|| {
                    AnthropicClient::batch(&crate::paths::LLM_CACHE_DB)
                        .expect("Failed to create Anthropic client")
                });
                llm_client.pending_batch_count()
            }
            TeacherBackend::Gpt52 => {
                let llm_client = OPENAI_CLIENT.get_or_init(|| {
                    OpenAiClient::batch(&crate::paths::LLM_CACHE_DB)
                        .expect("Failed to create OpenAI client")
                });
                llm_client.pending_batch_count()
            }
        },
        _ => Ok(0),
    }
}
