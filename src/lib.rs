// This is free and unencumbered software released into the public domain.

#![no_std]
#![forbid(unsafe_code)]

use asimov_module::{
    prelude::*,
    secrecy::{ExposeSecret, SecretString},
    tracing,
};
use core::error::Error;
use serde_json::{Value, json};

#[derive(Clone, Debug, bon::Builder)]
#[builder(on(String, into))]
pub struct Options {
    #[builder(default = "https://api.openai.com")]
    pub endpoint: String,

    #[builder(default = "gpt-5-mini")]
    pub model: String,

    #[builder(default = 1024)]
    pub max_tokens: usize,

    #[builder(into)]
    pub api_key: SecretString,
}

pub fn generate(input: impl AsRef<str>, options: &Options) -> Result<Vec<String>, Box<dyn Error>> {
    let req = json!({
        "model": options.model,
        "input": input.as_ref(),
    });

    let mut resp = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent("asimov-openai-module")
        .build()
        .new_agent()
        .post(format!("{}/v1/responses", options.endpoint))
        .header(
            "Authorization",
            format!("Bearer {}", options.api_key.expose_secret()),
        )
        .header("content-type", "application/json")
        .send_json(&req)
        .inspect_err(|e| tracing::error!("HTTP request failed: {e}"))?;
    tracing::debug!(response = ?resp);

    let status = resp.status();
    tracing::debug!(status = status.to_string());

    let resp: Value = resp
        .body_mut()
        .read_json()
        .inspect_err(|e| tracing::error!("unable to read HTTP response body: {e}"))?;
    tracing::debug!(body = ?resp);

    if !status.is_success() {
        tracing::error!("Received an error response: {status}");

        // {
        //   "error": {
        //     "message": "You exceeded your current quota, please check your plan and billing details. For more information on this error, read the docs: https://platform.openai.com/docs/guides/error-codes/api-errors.",
        //     "type": "insufficient_quota",
        //     "param": null,
        //     "code": "insufficient_quota"
        //   }
        // }
        if let Some(message) = resp["error"]["message"].as_str() {
            return Err(message.into());
        }
    }

    // {
    //   "id": "resp_...",
    //   "object": "response",
    //   "created_at": 1758184601,
    //   "status": "completed",
    //   "background": false,
    //   "billing": {
    //     "payer": "developer"
    //   },
    //   "error": null,
    //   "incomplete_details": null,
    //   "instructions": null,
    //   "max_output_tokens": null,
    //   "max_tool_calls": null,
    //   "model": "gpt-5-nano-2025-08-07",
    //   "output": [
    //     {
    //       "id": "rs_...",
    //       "type": "reasoning",
    //       "summary": []
    //     },
    //     {
    //       "id": "msg_...",
    //       "type": "message",
    //       "status": "completed",
    //       "content": [
    //         {
    //           "type": "output_text",
    //           "annotations": [],
    //           "logprobs": [],
    //           "text": "..."
    //         }
    //       ],
    //       "role": "assistant"
    //     }
    //   ],
    //   "parallel_tool_calls": true,
    //   "previous_response_id": null,
    //   "prompt_cache_key": null,
    //   "reasoning": {
    //     "effort": "medium",
    //     "summary": null
    //   },
    //   "safety_identifier": null,
    //   "service_tier": "default",
    //   "store": true,
    //   "temperature": 1.0,
    //   "text": {
    //     "format": {
    //       "type": "text"
    //     },
    //     "verbosity": "medium"
    //   },
    //   "tool_choice": "auto",
    //   "tools": [],
    //   "top_logprobs": 0,
    //   "top_p": 1.0,
    //   "truncation": "disabled",
    //   "usage": {
    //     "input_tokens": 7,
    //     "input_tokens_details": {
    //       "cached_tokens": 0
    //     },
    //     "output_tokens": 261,
    //     "output_tokens_details": {
    //       "reasoning_tokens": 192
    //     },
    //     "total_tokens": 268
    //   },
    //   "user": null,
    //   "metadata": {}
    // }

    let mut responses = Vec::new();

    if let Some(chunks) = resp["output"].as_array() {
        for chunk in chunks {
            if chunk["type"].as_str().is_none_or(|t| t != "message") {
                tracing::debug!("skipping non-message chunk in response: {chunk}");
                continue;
            }
            if chunk["role"].as_str().is_none_or(|r| r != "assistant") {
                tracing::debug!("skipping output chunk not from assistant: {chunk}");
                continue;
            }

            if let Some(chunk_contents) = chunk["content"].as_array() {
                for content in chunk_contents {
                    if content["type"].as_str().is_none_or(|r| r != "output_text") {
                        tracing::debug!("skipping non-text message chunk in response: {chunk}");
                        continue;
                    }

                    if let Some(text) = content["text"].as_str() {
                        responses.push(text.to_string());
                    }
                }
            };

            if let Some(status) = chunk["status"].as_str() {
                tracing::debug!(status);
            }
        }
    }

    Ok(responses)
}
