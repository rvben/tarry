use std::time::Duration;

use serde_json::{Value, json};

use crate::probe::{Probe, ProbeResult, Report};

pub struct JsonPathCheck {
    pub path: Vec<String>,
    pub expected: String,
}

/// Parses "a.b.0.c=val" into path segments and expected value.
pub fn parse_json_path(spec: &str) -> Result<JsonPathCheck, String> {
    let (path, expected) = spec
        .split_once('=')
        .ok_or_else(|| format!("--json-path '{spec}' must be <dot.path>=<value>"))?;
    if path.is_empty() {
        return Err(format!("--json-path '{spec}' has an empty path"));
    }
    Ok(JsonPathCheck {
        path: path.split('.').map(String::from).collect(),
        expected: expected.to_string(),
    })
}

fn lookup<'v>(value: &'v Value, path: &[String]) -> Option<&'v Value> {
    let mut current = value;
    for segment in path {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn value_matches(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(s) => s == expected,
        Value::Number(n) => n.to_string() == expected,
        Value::Bool(b) => b.to_string() == expected,
        Value::Null => expected == "null",
        Value::Array(_) | Value::Object(_) => false,
    }
}

pub struct HttpProbe {
    pub url: String,
    pub status: Option<u16>,
    pub contains: Vec<String>,
    pub json_paths: Vec<JsonPathCheck>,
    pub headers: Vec<(String, String)>,
    pub request_timeout: Duration,
}

impl Probe for HttpProbe {
    fn name(&self) -> &'static str {
        "http"
    }

    fn poll(&mut self) -> ProbeResult {
        let agent = ureq::AgentBuilder::new()
            .timeout(self.request_timeout)
            .build();
        let mut request = agent.get(&self.url);
        for (name, value) in &self.headers {
            request = request.set(name, value);
        }
        let (status, body) = match request.call() {
            Ok(resp) => {
                let status = resp.status();
                match resp.into_string() {
                    Ok(body) => (status, body),
                    Err(e) => {
                        return ProbeResult::Pending {
                            note: Some(format!("read body: {e}")),
                        };
                    }
                }
            }
            Err(ureq::Error::Status(status, resp)) => {
                (status, resp.into_string().unwrap_or_default())
            }
            Err(e) => {
                return ProbeResult::Pending {
                    note: Some(format!("request: {e}")),
                };
            }
        };

        let status_ok = match self.status {
            Some(expected) => status == expected,
            None => (200..300).contains(&status),
        };
        if !status_ok {
            return ProbeResult::Pending {
                note: Some(format!("status {status}")),
            };
        }
        for needle in &self.contains {
            if !body.contains(needle) {
                return ProbeResult::Pending {
                    note: Some(format!("body does not contain '{needle}'")),
                };
            }
        }
        if !self.json_paths.is_empty() {
            let parsed: Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    return ProbeResult::Pending {
                        note: Some(format!("body is not JSON: {e}")),
                    };
                }
            };
            for check in &self.json_paths {
                let found = lookup(&parsed, &check.path);
                let matched = found.is_some_and(|v| value_matches(v, &check.expected));
                if !matched {
                    return ProbeResult::Pending {
                        note: Some(format!(
                            "json path {} != {}",
                            check.path.join("."),
                            check.expected
                        )),
                    };
                }
            }
        }
        ProbeResult::Met(Report {
            summary: format!("{} returned {status} and matched", self.url),
            detail: json!({"url": self.url, "status": status}),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn probe(url: String) -> HttpProbe {
        HttpProbe {
            url,
            status: None,
            contains: vec![],
            json_paths: vec![],
            headers: vec![],
            request_timeout: Duration::from_secs(5),
        }
    }

    #[test]
    fn ok_status_is_met() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/health");
            then.status(200).body("ok");
        });
        let mut p = probe(server.url("/health"));
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn non_2xx_is_pending_by_default() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/health");
            then.status(503);
        });
        let mut p = probe(server.url("/health"));
        match p.poll() {
            ProbeResult::Pending { note } => assert!(note.unwrap().contains("503")),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn explicit_status_must_match_exactly() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/gone");
            then.status(404);
        });
        let mut p = probe(server.url("/gone"));
        p.status = Some(404);
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn contains_matcher_checks_body() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/v");
            then.status(200).body("version 1.2.2");
        });
        let mut p = probe(server.url("/v"));
        p.contains = vec!["1.2.3".into()];
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn json_path_matcher_checks_body() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/status");
            then.status(200)
                .json_body(serde_json::json!({"jobs": [{"state": "done"}], "count": 1}));
        });
        let mut p = probe(server.url("/status"));
        p.json_paths = vec![
            parse_json_path("jobs.0.state=done").unwrap(),
            parse_json_path("count=1").unwrap(),
        ];
        assert!(matches!(p.poll(), ProbeResult::Met(_)));
    }

    #[test]
    fn json_path_mismatch_is_pending() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/status");
            then.status(200)
                .json_body(serde_json::json!({"state": "running"}));
        });
        let mut p = probe(server.url("/status"));
        p.json_paths = vec![parse_json_path("state=ready").unwrap()];
        assert!(matches!(p.poll(), ProbeResult::Pending { .. }));
    }

    #[test]
    fn connection_error_is_pending() {
        let mut p = probe("http://127.0.0.1:1/x".into());
        match p.poll() {
            ProbeResult::Pending { note } => assert!(note.is_some()),
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn parse_json_path_rejects_missing_equals() {
        assert!(parse_json_path("no-equals-here").is_err());
    }
}
