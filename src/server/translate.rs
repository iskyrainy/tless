use anyhow::Result;
use reqwest::Client;

trait TranslationBackend {
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse>;
}

struct TranslationRequest<'a> {
    system_prompt: &'static str,
    stable_prefix: &'a str,
    target_lang: String,
}

struct TranslationResponse {
    text: String,
    usage: Option<Usage>,
}

#[derive(Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_tokens: u32,
    pub cache_miss_tokens: Option<u32>,
}

enum Provider {
    OpenAI,
    Deepseek,
    Kimi,
    Qwen,
    Glm,
}

struct OpenaiCompletionProvider {
    client: Client,
    provider: Provider,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenaiCompletionProvider {
    fn new<T: Into<String>>(provider: Provider, api_key: T, base_url: T, model: T) -> Self {
        Self {
            client: reqwest::Client::new(),
            provider,
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

impl TranslationBackend for OpenaiCompletionProvider {
    async fn translate(&self, req: TranslationRequest<'_>) -> Result<TranslationResponse> {
        todo!()
    }
}
