use super::redact_line;

#[test]
fn redact_api_key_eq_value() {
    assert_eq!(
        redact_line("api_key=sk-abc123def456"),
        "api_key=[REDACTED]",
    );
}

#[test]
fn redact_api_key_colon_value() {
    assert_eq!(
        redact_line(r#""api_key": "sk-abc123def456""#),
        r#""api_key": [REDACTED]"#,
    );
}

#[test]
fn redact_apikey_uppercase() {
    assert_eq!(
        redact_line("APIKEY=ghp_abc123def456ghi789"),
        "APIKEY=[REDACTED]",
    );
}

#[test]
fn redact_api_key_with_whitespace() {
    assert_eq!(
        redact_line("api_key = secret-value-here"),
        "api_key = [REDACTED]",
    );
}

#[test]
fn redact_secret_eq_value() {
    assert_eq!(
        redact_line("MY_SECRET=super-secret-passphrase"),
        "MY_SECRET=[REDACTED]",
    );
}

#[test]
fn redact_password_eq_value() {
    assert_eq!(
        redact_line("password=hunter2"),
        "password=[REDACTED]",
    );
}

#[test]
fn redact_passwd_eq_value() {
    assert_eq!(
        redact_line("passwd=hunter2"),
        "passwd=[REDACTED]",
    );
}

#[test]
fn redact_private_key() {
    assert_eq!(
        redact_line("private_key=-----BEGIN RSA PRIVATE KEY-----"),
        "private_key=[REDACTED]",
    );
}

#[test]
fn redact_auth_token() {
    assert_eq!(
        redact_line("auth_token=abc123def456ghi789"),
        "auth_token=[REDACTED]",
    );
}

#[test]
fn redact_bearer_authorization() {
    assert_eq!(
        redact_line("Authorization: Bearer sk-abc123def456ghi789"),
        "Authorization: Bearer [REDACTED]",
    );
}

#[test]
fn redact_bearer_lowercase() {
    assert_eq!(
        redact_line("authorization: bearer eyJhbGciOiJIUzI1NiJ9"),
        "authorization: bearer [REDACTED]",
    );
}

#[test]
fn redact_openai_api_key_format() {
    assert_eq!(
        redact_line("sk-proj-abc123def456ghi789jkl012mno345pqr678stu901vwx234"),
        "[REDACTED]",
    );
}

#[test]
fn redact_github_pat() {
    assert_eq!(redact_line("ghp_abc123def456ghi789jkl012mno345"), "[REDACTED]");
}

#[test]
fn redact_github_oauth() {
    assert_eq!(redact_line("gho_abc123def456ghi789jkl012mno345"), "[REDACTED]");
}

#[test]
fn redact_github_user_token() {
    assert_eq!(redact_line("ghu_abc123def456ghi789jkl012mno345"), "[REDACTED]");
}

#[test]
fn redact_github_server_token() {
    assert_eq!(redact_line("ghs_abc123def456ghi789jkl012mno345"), "[REDACTED]");
}

#[test]
fn redact_github_refresh_token() {
    assert_eq!(redact_line("ghr_abc123def456ghi789jkl012mno345"), "[REDACTED]");
}

#[test]
fn redact_url_query_api_key() {
    assert_eq!(
        redact_line("https://api.example.com/v1?api_key=sk-abc123def456"),
        "https://api.example.com/v1?api_key=[REDACTED]",
    );
}

#[test]
fn redact_url_query_token() {
    assert_eq!(
        redact_line("https://api.example.com/v1?token=abc123&user=me"),
        "https://api.example.com/v1?token=[REDACTED]&user=me",
    );
}

#[test]
fn redact_url_query_password() {
    assert_eq!(
        redact_line("https://api.example.com/login?password=hunter2"),
        "https://api.example.com/login?password=[REDACTED]",
    );
}

#[test]
fn redact_url_query_auth() {
    assert_eq!(
        redact_line("https://api.example.com/auth?auth=basic&user=me"),
        "https://api.example.com/auth?auth=[REDACTED]&user=me",
    );
}

#[test]
fn redact_url_query_secret() {
    assert_eq!(
        redact_line("https://api.example.com/data?secret=my-secret-key"),
        "https://api.example.com/data?secret=[REDACTED]",
    );
}

#[test]
fn redact_url_query_apikey() {
    assert_eq!(
        redact_line("https://api.example.com/data?apikey=abc123"),
        "https://api.example.com/data?apikey=[REDACTED]",
    );
}

#[test]
fn passes_through_innocent_lines() {
    let lines = [
        "Starting MCP server...",
        "Connected to client",
        "Tool 'get_weather' called with args: {\"city\": \"London\"}",
        "Response sent successfully",
        "22:14:33 [info] Handling request",
        "total_tokens=1500, completion_tokens=450",
        "token_count=1234",
        "user_authenticated=true",
        "Using model: gpt-4o",
        "Listening on stdin...",
        "[metrics] latency_ms=42, errors=0",
        "some_random_key=just_a_value",
    ];
    for line in &lines {
        assert_eq!(redact_line(line), *line, "unexpected redaction: {line}");
    }
}

#[test]
fn redacts_multiple_secrets_in_one_line() {
    let input = "api_key=sk-abc secret=hunter2 Authorization: Bearer ghp_def456";
    let expected = "api_key=[REDACTED] secret=[REDACTED] Authorization: Bearer [REDACTED]";
    assert_eq!(redact_line(input), expected);
}

#[test]
fn redact_empty_string() {
    assert_eq!(redact_line(""), "");
}

#[test]
fn redact_api_key_with_dash() {
    assert_eq!(redact_line("api-key=my-secret-key"), "api-key=[REDACTED]");
}

#[test]
fn redact_api_key_underscore() {
    assert_eq!(redact_line("api_key=my-secret-key"), "api_key=[REDACTED]");
}

#[test]
fn redact_api_key_no_separator_is_noop() {
    // Without '=' or ':', the value should not be matched (no false positive).
    let line = "dump_of_api_key_printout";
    assert_eq!(redact_line(line), line);
}

#[test]
fn redact_json_body_with_secrets() {
    let line = r#"{"api_key":"sk-abc123def456","model":"gpt-4"}"#;
    let expected = r#"{"api_key":[REDACTED],"model":"gpt-4"}"#;
    assert_eq!(redact_line(line), expected);
}

#[test]
fn redact_shorter_github_pat_does_not_match() {
    // GitHub PATs with < 16 chars after prefix should not be redacted
    // (too short to be a real token).
    let line = "ghp_abc123";
    assert_eq!(redact_line(line), line);
}

#[test]
fn redact_short_openai_key_does_not_match() {
    // OpenAI keys with < 20 chars after prefix should not be redacted.
    let line = "sk-abc123def";
    assert_eq!(redact_line(line), line);
}