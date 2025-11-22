// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]

use anyhow::{Context as _, Result, anyhow};
use asimov_module::{
    prelude::*,
    secrecy::{ExposeSecret, SecretString},
    tracing,
};
use serde_json::{Value, json};

#[derive(Clone, Debug, bon::Builder)]
#[builder(on(String, into))]
pub struct Options {
    #[builder(default = "https://api.openai.com")]
    pub endpoint: String,

    #[builder(default = "gpt-5-mini")]
    pub model: String,

    pub max_tokens: Option<usize>,

    #[builder(into)]
    pub api_key: SecretString,
}

pub fn generate(input: impl AsRef<str>, options: &Options) -> Result<Vec<String>> {
    let mut req = json!({
        "model": options.model,
        "messages": [{
            "role": "user",
            "content": input.as_ref(),
        }],
    });

    if let Some(max_tokens) = options.max_tokens {
        req["max_completion_tokens"] = max_tokens.into();
    }

    let mut resp = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent("asimov-openai-module")
        .build()
        .new_agent()
        .post(format!("{}/v1/chat/completions", options.endpoint))
        .header(
            "Authorization",
            format!("Bearer {}", options.api_key.expose_secret()),
        )
        .header("content-type", "application/json")
        .send_json(&req)
        .context("HTTP request failed")?;
    tracing::debug!(response = ?resp);

    let status = resp.status();

    let resp: Value = resp
        .body_mut()
        .read_json()
        .inspect_err(|e| tracing::error!("unable to read HTTP response body: {e}"))?;
    tracing::debug!(response = %resp);

    if !status.is_success() {
        tracing::debug!(%status, "Received an unsuccessful response");

        // {
        //   "error": {
        //     "message": "You exceeded your current quota, please check your plan and billing details. For more information on this error, read the docs: https://platform.openai.com/docs/guides/error-codes/api-errors.",
        //     "type": "insufficient_quota",
        //     "param": null,
        //     "code": "insufficient_quota"
        //   }
        // }
        if let Some(message) = resp["error"]["message"].as_str() {
            return Err(anyhow!(message.to_string()));
        }
    }

    // {
    //   "id": "chatcmpl-...",
    //   "object": "chat.completion",
    //   "created": 1741569952,
    //   "model": "gpt-4.1-2025-04-14",
    //   "choices": [
    //     {
    //       "index": 0,
    //       "message": {
    //         "role": "assistant",
    //         "content": "...",
    //         "refusal": null,
    //         "annotations": []
    //       },
    //       "logprobs": null,
    //       "finish_reason": "stop"
    //     }
    //   ],
    //   "usage": {
    //     "prompt_tokens": 19,
    //     "completion_tokens": 10,
    //     "total_tokens": 29,
    //     "prompt_tokens_details": {
    //       "cached_tokens": 0,
    //       "audio_tokens": 0
    //     },
    //     "completion_tokens_details": {
    //       "reasoning_tokens": 0,
    //       "audio_tokens": 0,
    //       "accepted_prediction_tokens": 0,
    //       "rejected_prediction_tokens": 0
    //     }
    //   },
    //   "service_tier": "default"
    // }

    let mut responses = Vec::new();

    if let Some(choices) = resp["choices"].as_array() {
        // there is only one "choice" if the request doesn't have an "n" parameter
        for choice in choices {
            if choice["message"]["role"]
                .as_str()
                .is_none_or(|r| r != "assistant")
            {
                tracing::debug!("skipping output not from assistant: {choice}");
                continue;
            }

            if let Some(content) = choice["message"]["content"].as_str() {
                responses.push(content.to_string())
            } else if let Some(refusal) = choice["message"]["refusal"].as_str() {
                tracing::error!("Request refused: {refusal}")
            }

            if let Some(finish_reason) = choice["finish_reason"].as_str() {
                tracing::debug!(finish_reason);
            }
        }
    }

    Ok(responses)
}
