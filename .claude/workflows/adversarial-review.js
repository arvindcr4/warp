export const meta = {
  name: 'adversarial-review',
  description: '20-agent adversarial codebase review across security, architecture, correctness, and more',
  phases: [
    { title: 'Review', detail: '20 parallel adversarial reviewers' },
    { title: 'Synthesize', detail: 'Aggregate findings into ranked report' },
  ],
}

const lenses = [
  // Security (4)
  { name: 'sec-secrets', phase: 'Review', prompt: `Adversarially review the warp-minimax codebase at /Users/arvind/warp-minimax for:
1. Hardcoded secrets, API keys, tokens in source code
2. Overly permissive CORS or security headers
3. Insecure default configurations
4. Credential leakage in logs or error messages
Search broadly across .rs, .ts, .js, .env, .toml, .json files. Look for patterns like "api_key", "token", "secret", "password", "auth_token" in string literals.` },
  { name: 'sec-input', phase: 'Review', prompt: `Adversarially review the warp-minimax codebase at /Users/arvind/warp-minimax for input validation vulnerabilities:
1. SQL injection in persistence/diesel queries
2. Command injection from user input
3. Path traversal in file operations
4. Prototype pollution or injection in JS/TS code
5. Unsafe deserialization (serde, JSON parsing)
Focus on crates that handle external/user input: ai/, graphql/, warp_local_server/, persistence/` },
  { name: 'sec-supply-chain', phase: 'Review', prompt: `Adversarially review the warp-minimax codebase at /Users/arvind/warp-minimax for supply chain and dependency risks:
1. Outdated dependencies with known CVEs
2. Unpinned dependency versions in Cargo.toml
3. Overly broad dependency permissions
4. Git dependencies pointing to forks (supply chain risk)
5. Unsafe or suspicious build.rs scripts
Check Cargo.toml, package.json, Cargo.lock files.` },
  // Architecture (4)
  { name: 'arch-bridge', phase: 'Review', prompt: `Adversarially review the AI bridge architecture in /Users/arvind/warp-minimax, particularly crates/ai/warp_local_server/anthropic_bridge/:
1. Does the Anthropic-to-MiniMax mapping preserve semantic correctness?
2. Are error status codes correctly translated?
3. Are timeouts properly propagated across providers?
4. Is there any leaky abstraction where MiniMax specifics leak into generic code?
5. Can provider-specific quirks cause silent correctness bugs?
6. How is streaming handled differently between providers?
Read all files in anthropic_bridge/ and any provider abstraction layer.` },
  { name: 'arch-plugins', phase: 'Review', prompt: `Adversarially review the plugin/module/tool system in /Users/arvind/warp-minimax:
1. How are MCP tools registered and invoked?
2. Is there proper sandboxing between tool execution and the host?
3. Can tools access resources they shouldn't?
4. How are tool results validated before being returned?
5. Is there any shared mutable state that tools could corrupt?
Search for "tool", "plugin", "mcp", "register", "execute" in crates/ai/ and the main app crate.` },
  { name: 'arch-concurrency', phase: 'Review', prompt: `Adversarially review concurrency patterns in /Users/arvind/warp-minimax:
1. Look for .unwrap() on Mutex/RwLock locks — could deadlock
2. Check for async fn that calls std::sync::Mutex (will deadlock in async context)
3. Look for tokio::spawn without JoinHandle
4. Check for unbounded channels that could grow forever
5. Search for unsafe Send/Sync impls
6. Look for RefCell/RwLock usage without proper guards in async code
Search .rs files for "unwrap\\(\\)" on locks, "Mutex", "RwLock", "unsafe impl Send", "tokio::spawn", "mpsc::unbounded"` },
  // Correctness (4)
  { name: 'err-handling', phase: 'Review', prompt: `Adversarially review error handling in /Users/arvind/warp-minimax:
1. Find all production .unwrap() and .expect() calls that could panic at runtime
2. Check for silently swallowed errors (let _ = result;)
3. Look for overly broad catch-all error patterns
4. Check if error messages are helpful and contextual
5. Find places where errors fail to propagate correctly
6. Look for panic!() in library code that should return Result
Focus on .unwrap(), .expect(), panic!(), todo!(), unimplemented!() calls` },
  { name: 'data-races', phase: 'Review', prompt: `Adversarially review the warp-minimax codebase at /Users/arvind/warp-minimax for data races and memory safety:
1. Search for shared mutable state behind Rc/RefCell (not thread-safe)
2. Look for global statics or lazy_static that are mutated
3. Check for unsafe code blocks — what invariants do they assume?
4. Look for incorrect atomic ordering (Relaxed where Acquire/Release needed)
5. Check for interior mutability patterns that break thread safety
Search: "unsafe", "Rc<RefCell", "static mut", "Ordering::Relaxed", "lazy_static", "once_cell"` },
  { name: 'persistence', phase: 'Review', prompt: `Adversarially review the persistence/database layer in /Users/arvind/warp-minimax (look in crates/persistence/ and any SQL/diesel code):
1. Check for raw SQL strings that could be injection vectors
2. Look for missing or incorrect migrations
3. Check for N+1 query patterns
4. Look for improper transaction boundaries
5. Check for connection pool exhaustion risks
6. Check for schema inconsistencies between migrations and models
Search for diesel::sql_query, sql_literal, .expect() in db code, transaction patterns` },
  // AI/Agent (4)
  { name: 'ai-safety', phase: 'Review', prompt: `Adversarially review the AI agent safety in /Users/arvind/warp-minimax:
1. How are tool call results sanitized before being shown to the user?
2. Is there any risk of prompt injection from tool outputs?
3. How are system prompts constructed — could user input contaminate them?
4. Is there rate limiting on AI API calls?
5. What happens if the AI provider returns malicious content?
6. Are there safeguards against infinite agent loops?
Search in crates/ai/, anthropic_bridge/, warp_local_server/ for prompt construction, tool handling, rate limiting` },
  { name: 'ai-streaming', phase: 'Review', prompt: `Adversarially review AI streaming and event handling in /Users/arvind/warp-minimax:
1. Check SSE event stream parsing for correctness
2. Look for buffer overflow or unbounded growth in streaming responses
3. Check how stream cancellation works — can we leak resources?
4. Look for race conditions between stream events and state mutations
5. Check for proper backpressure in streaming pipelines
6. How are partial responses handled on error/disconnect?
Search for "stream", "sse", "event", "chunk", "response" in crates/ai/` },
  { name: 'feature-flags', phase: 'Review', prompt: `Adversarially review the feature flag system in /Users/arvind/warp-minimax:
1. How are feature flags evaluated — runtime or compile-time?
2. Can stale flags cause dead code or maintenance burden?
3. Are there feature flags that gate security-critical features?
4. Check for feature flag toggling at runtime vs compile-time inconsistencies
5. Look for feature flag test coverage
6. Is there a clear lifecycle for flag removal?
Search for "FeatureFlag", "feature_flag", "flag", "ff_" patterns` },
  // Testing & Quality (4)
  { name: 'test-coverage', phase: 'Review', prompt: `Adversarially review test coverage in /Users/arvind/warp-minimax:
1. Which crates/modules have no tests at all?
2. Are there #[cfg(test)] modules that are stale or trivial?
3. Do critical paths (auth, payments, data access) have tests?
4. Are there tests that don't actually assert anything?
5. Look for mock-heavy tests that test mocks instead of real behavior
6. Check if integration tests cover E2E flows
Search for "#[test]", "#[cfg(test)]", "assert_eq", "assert!" patterns` },
  { name: 'perf-bottlenecks', phase: 'Review', prompt: `Adversarially review performance in /Users/arvind/warp-minimax:
1. Search for clone() on large data structures in hot paths
2. Look for unnecessary allocations (format! in loops, String concat)
3. Check for O(n²) patterns in search/iteration
4. Look for Arc<String> vs Arc<str> patterns (suboptimal)
5. Check for serial bottlenecks in the event loop
6. Look for oversized enum variants
Search for ".clone()", "format!", "to_string()", "to_owned()" in performance-sensitive areas` },
  { name: 'logging', phase: 'Review', prompt: `Adversarially review logging and observability in /Users/arvind/warp-minimax:
1. Are sensitive values ever logged (tokens, keys, passwords)?
2. Is there structured logging vs ad-hoc println/eprintln?
3. Are error paths properly instrumented?
4. Can log volume cause performance issues?
5. Is there proper span/trace correlation across async boundaries?
6. Are there debugging println! statements left in production code?
Search for "println", "eprintln", "dbg!", "log::", "tracing::", "info!", "error!", "warn!"` },
  // Platform-specific (2)
  { name: 'macos-safety', phase: 'Review', prompt: `Adversarially review macOS-specific code in /Users/arvind/warp-minimax:
1. Check the Obj-C bridge for memory leaks (retain/release balance)
2. Look for improper main-thread dispatch assumptions
3. Check for macOS sandbox compliance issues
4. Look for hardcoded macOS paths or assumptions
5. Check proper handling of app lifecycle events
Search for .m, .mm, objc, cocoa, nsstring, NSNotification patterns` },
  { name: 'build-system', phase: 'Review', prompt: `Adversarially review the build system in /Users/arvind/warp-minimax:
1. Check Cargo.toml for workspace misconfigurations
2. Look for features that should be additive but aren't
3. Check for duplicate or conflicting dependencies
4. Look for non-portable build scripts or platform-specific hacks
5. Check if build.rs scripts could have side effects
6. Look for cfg() spaghetti that's hard to verify
Read Cargo.toml files, build.rs files, and any Makefile/build scripts` },
]

phase('Review')
const results = await parallel(
  lenses.map(lens => () =>
    agent(lens.prompt, {
      label: lens.name,
      phase: lens.phase,
      schema: {
        type: 'object',
        properties: {
          findings: {
            type: 'array',
            items: {
              type: 'object',
              properties: {
                severity: { type: 'string', enum: ['CRITICAL', 'HIGH', 'MEDIUM', 'LOW', 'INFO'] },
                title: { type: 'string' },
                description: { type: 'string' },
                fileOrPattern: { type: 'string' },
                impact: { type: 'string' },
                recommendation: { type: 'string' },
              },
              required: ['severity', 'title', 'description', 'recommendation'],
            },
          },
        },
        required: ['findings'],
      },
    })
  )
)

phase('Synthesize')
const allFindings = results
  .filter(Boolean)
  .flatMap(r => r.findings)
  .filter(f => f)

const severityOrder = { CRITICAL: 0, HIGH: 1, MEDIUM: 2, LOW: 3, INFO: 4 }
const ranked = [...allFindings].sort((a, b) => (severityOrder[a.severity] ?? 9) - (severityOrder[b.severity] ?? 9))

const summary = {
  total: ranked.length,
  bySeverity: {
    CRITICAL: ranked.filter(f => f.severity === 'CRITICAL').length,
    HIGH: ranked.filter(f => f.severity === 'HIGH').length,
    MEDIUM: ranked.filter(f => f.severity === 'MEDIUM').length,
    LOW: ranked.filter(f => f.severity === 'LOW').length,
    INFO: ranked.filter(f => f.severity === 'INFO').length,
  },
  findings: ranked,
}

log(`Total findings: ${summary.total}`)
log(`Critical: ${summary.bySeverity.CRITICAL}, High: ${summary.bySeverity.HIGH}, Medium: ${summary.bySeverity.MEDIUM}, Low: ${summary.bySeverity.LOW}, Info: ${summary.bySeverity.INFO}`)

return summary