export const meta = {
  name: 'review-and-fix',
  description: '12 adversarial review agents + 20 fix agents in a single workflow (Critical+High+Medium)',
  phases: [
    { title: 'Review', detail: '12 parallel adversarial reviewers' },
    { title: 'Fix', detail: '20 parallel fix agents on disjoint file sets' },
  ],
}

const reviewLenses = [
  {
    name: 'sec',
    phase: 'Review',
    prompt: `Adversarially review the warp-minimax codebase at /Users/arvind/warp-minimax for security issues:
1. Hardcoded secrets, API keys, tokens, passwords in source code or config files
2. SQL injection in diesel queries (raw SQL strings, sql_query, sql_literal)
3. Command injection from user input
4. Path traversal in file operations
5. Unsafe deserialization (untyped serde, JSON parsing of untrusted input)
6. Outdated dependencies with known CVEs
7. Unpinned dependency versions in Cargo.toml
8. Git dependencies pointing to forks (supply chain risk)
9. Suspicious build.rs scripts

Search across .rs, .ts, .js, .env, .toml, .json files. Focus on crates/persistence, ai/, anthropic_bridge/, warp_local_server/, mcp/, http_server/, graphql/.

For each finding, report: severity (CRITICAL/HIGH/MEDIUM/LOW/INFO), title, description, file path, impact, recommendation. Be specific - cite line numbers when possible.`,
  },
  {
    name: 'arch-bridge',
    phase: 'Review',
    prompt: `Adversarially review the AI bridge architecture in /Users/arvind/warp-minimax (crates/ai/, anthropic_bridge/, warp_local_server/):
1. Does the Anthropic-to-MiniMax mapping preserve semantic correctness?
2. Are error status codes correctly translated?
3. Are timeouts properly propagated across providers?
4. Is there any leaky abstraction where MiniMax specifics leak into generic code?
5. Can provider-specific quirks cause silent correctness bugs?
6. How is streaming handled differently between providers?

Read all files in anthropic_bridge/ and any provider abstraction layer. Cite file paths, severity, recommendation.`,
  },
  {
    name: 'arch-plugins',
    phase: 'Review',
    prompt: `Adversarially review the plugin/MCP tool system in /Users/arvind/warp-minimax:
1. How are MCP tools registered and invoked?
2. Is there proper sandboxing between tool execution and the host?
3. Can tools access resources they shouldn't?
4. How are tool results validated before being returned?
5. Is there any shared mutable state that tools could corrupt?
6. JSON-RPC parsing: depth limits, type validation, error handling.

Search for "tool", "plugin", "mcp", "register", "execute", "jsonrpc" in crates/ai/, mcp/, jsonrpc/. Cite files.`,
  },
  {
    name: 'concurrency',
    phase: 'Review',
    prompt: `Adversarially review concurrency patterns in /Users/arvind/warp-minimax:
1. .unwrap() on Mutex/RwLock locks — could deadlock
2. async fn that calls std::sync::Mutex (will deadlock in async context)
3. tokio::spawn without JoinHandle
4. Unbounded channels (mpsc::unbounded) that could grow forever
5. unsafe impl Send/Sync
6. RefCell/RwLock usage without proper guards in async code

Search .rs files for "unwrap\\(\\)" on locks, "Mutex", "RwLock", "unsafe impl Send", "tokio::spawn", "mpsc::unbounded". Cite file/line, severity, recommendation.`,
  },
  {
    name: 'err-handling',
    phase: 'Review',
    prompt: `Adversarially review error handling in /Users/arvind/warp-minimax:
1. Find production .unwrap() and .expect() calls that could panic at runtime
2. Check for silently swallowed errors (let _ = result;)
3. Look for overly broad catch-all error patterns
4. Check if error messages are helpful and contextual
5. Find places where errors fail to propagate correctly
6. Look for panic!() in library code that should return Result

Focus on .unwrap(), .expect(), panic!(), todo!(), unimplemented!() calls in crates/*/src and app/src. Skip test files. For each risky call, judge if it's safe (test code, after explicit check) or risky. Report severity, file path, line number, recommendation.`,
  },
  {
    name: 'data-races',
    phase: 'Review',
    prompt: `Adversarially review the warp-minimax codebase at /Users/arvind/warp-minimax for data races and memory safety:
1. Search for shared mutable state behind Rc/RefCell (not thread-safe)
2. Look for global statics or lazy_static that are mutated
3. Check for unsafe code blocks — what invariants do they assume?
4. Look for incorrect atomic ordering (Relaxed where Acquire/Release needed)
5. Check for interior mutability patterns that break thread safety
6. std::sync::Mutex held across .await points

Search: "unsafe", "Rc<RefCell", "static mut", "Ordering::Relaxed", "lazy_static", "once_cell". Cite file/line, severity, recommendation.`,
  },
  {
    name: 'persistence',
    phase: 'Review',
    prompt: `Adversarially review the persistence/database layer in /Users/arvind/warp-minimax:
1. Check for raw SQL strings that could be injection vectors (diesel::sql_query, sql_literal)
2. Look for missing or incorrect migrations
3. Check for N+1 query patterns
4. Look for improper transaction boundaries
5. Check for connection pool exhaustion risks
6. Check for schema inconsistencies between migrations and models
7. Long-running queries without timeouts

Look in crates/persistence/, app/src/, any diesel:: usage. Cite file, severity, recommendation.`,
  },
  {
    name: 'ai-safety',
    phase: 'Review',
    prompt: `Adversarially review the AI agent safety in /Users/arvind/warp-minimax:
1. How are tool call results sanitized before being shown to the user?
2. Is there any risk of prompt injection from tool outputs?
3. How are system prompts constructed — could user input contaminate them?
4. Is there rate limiting on AI API calls?
5. What happens if the AI provider returns malicious content?
6. Are there safeguards against infinite agent loops?
7. Auth/token handling in API calls

Search in crates/ai/, anthropic_bridge/, warp_local_server/ for prompt construction, tool handling, rate limiting. Cite files, severity, recommendation.`,
  },
  {
    name: 'ai-streaming',
    phase: 'Review',
    prompt: `Adversarially review AI streaming and event handling in /Users/arvind/warp-minimax:
1. Check SSE event stream parsing for correctness
2. Look for buffer overflow or unbounded growth in streaming responses
3. Check how stream cancellation works — can we leak resources?
4. Look for race conditions between stream events and state mutations
5. Check for proper backpressure in streaming pipelines
6. How are partial responses handled on error/disconnect?

Search for "stream", "sse", "event", "chunk", "response" in crates/ai/. Cite file, severity, recommendation.`,
  },
  {
    name: 'test-coverage',
    phase: 'Review',
    prompt: `Adversarially review test coverage in /Users/arvind/warp-minimax:
1. Which crates/modules have no tests at all?
2. Are there #[cfg(test)] modules that are stale or trivial?
3. Do critical paths (auth, payments, data access) have tests?
4. Are there tests that don't actually assert anything?
5. Look for mock-heavy tests that test mocks instead of real behavior
6. Check if integration tests cover E2E flows

Search for "#[test]", "#[cfg(test)]", "assert_eq", "assert!" patterns. For each gap, cite file, severity, recommendation.`,
  },
  {
    name: 'perf',
    phase: 'Review',
    prompt: `Adversarially review performance in /Users/arvind/warp-minimax:
1. Search for clone() on large data structures in hot paths
2. Look for unnecessary allocations (format! in loops, String concat)
3. Check for O(n²) patterns in search/iteration
4. Look for Arc<String> vs Arc<str> patterns (suboptimal)
5. Check for serial bottlenecks in the event loop
6. Look for oversized enum variants

Search for ".clone()", "format!", "to_string()", "to_owned()" in performance-sensitive areas. Cite file/function, severity, recommendation.`,
  },
  {
    name: 'logging',
    phase: 'Review',
    prompt: `Adversarially review logging and observability in /Users/arvind/warp-minimax:
1. Are sensitive values ever logged (tokens, keys, passwords)?
2. Is there structured logging vs ad-hoc println/eprintln?
3. Are error paths properly instrumented?
4. Can log volume cause performance issues?
5. Is there proper span/trace correlation across async boundaries?
6. Are there debugging println! statements left in production code?

Search for "println", "eprintln", "dbg!", "log::", "tracing::", "info!", "error!", "warn!". Cite file, severity, recommendation.`,
  },
]

const FINDING_SCHEMA = {
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
        required: ['severity', 'title', 'description', 'recommendation', 'fileOrPattern'],
      },
    },
  },
  required: ['findings'],
}

phase('Review')
const reviewResults = await parallel(
  reviewLenses.map(lens => () =>
    agent(lens.prompt, {
      label: lens.name,
      phase: lens.phase,
      schema: FINDING_SCHEMA,
    })
  )
)

const allFindings = reviewResults
  .filter(Boolean)
  .flatMap(r => r.findings || [])
  .filter(f => f && (f.severity === 'CRITICAL' || f.severity === 'HIGH' || f.severity === 'MEDIUM'))

const severityOrder = { CRITICAL: 0, HIGH: 1, MEDIUM: 2 }
allFindings.sort((a, b) => (severityOrder[a.severity] ?? 9) - (severityOrder[b.severity] ?? 9))

log(`Review complete: ${allFindings.length} fixable findings (Critical+High+Medium)`)

if (allFindings.length === 0) {
  log('No Critical/High/Medium findings - skipping fix phase')
  return { findings: [], fixes: [] }
}

// Group findings by file, then pack into N buckets ensuring disjoint file sets
function clusterByFile(findings, numBuckets) {
  const fileGroups = {}
  for (const f of findings) {
    const file = f.fileOrPattern || 'unknown'
    if (!fileGroups[file]) fileGroups[file] = []
    fileGroups[file].push(f)
  }
  // Sort by size desc - biggest first
  const files = Object.entries(fileGroups).sort((a, b) => b[1].length - a[1].length)
  // Greedy: assign each file-group to bucket with fewest items
  const buckets = Array.from({length: numBuckets}, () => ({ files: [], findings: [] }))
  for (const [file, items] of files) {
    let minIdx = 0
    for (let i = 1; i < buckets.length; i++) {
      if (buckets[i].findings.length < buckets[minIdx].findings.length) minIdx = i
    }
    buckets[minIdx].files.push(file)
    buckets[minIdx].findings.push(...items)
  }
  return buckets.filter(b => b.findings.length > 0).map((b, i) => ({
    label: `fix-${i}-${(b.files[0] || 'misc').split('/').slice(-1)[0]}`,
    files: b.files,
    findings: b.findings,
  }))
}

const targetBuckets = Math.min(20, allFindings.length)
const fixBuckets = clusterByFile(allFindings, targetBuckets)
log(`Created ${fixBuckets.length} fix buckets (target ${targetBuckets})`)

phase('Fix')
const fixResults = await parallel(
  fixBuckets.map(bucket => () =>
    agent(
      `You are a senior Rust/TypeScript engineer fixing issues found by adversarial review. Each finding has severity, description, file path, and recommendation. Apply the MINIMAL CORRECT FIX for each finding. Do NOT refactor unrelated code. Do NOT introduce regressions.

You are responsible ONLY for these files (do not edit anything else):
${bucket.files.map(f => '- ' + f).join('\n')}

If a fix requires touching other files, mark the finding as 'skipped' and explain why.

Findings to fix:
${JSON.stringify(bucket.findings, null, 2)}

Repo: /Users/arvind/warp-minimax

Workflow:
1. Read each file fully before editing
2. Apply the minimal fix matching the recommendation
3. For Rust changes in a crate, run \`cargo check -p <crate>\` (or \`cargo check --workspace\` for workspace) to verify compilation
4. For TS/JS changes, run \`pnpm tsc --noEmit\` from the relevant package
5. If a fix breaks compilation, REVERT it and mark as skipped with reason
6. Prefer surgical edits over rewrites

Report: list of fixed finding titles, list of skipped finding titles, build status, notes.`,
      {
        label: bucket.label,
        phase: 'Fix',
        schema: {
          type: 'object',
          properties: {
            fixed: { type: 'array', items: { type: 'string' } },
            skipped: { type: 'array', items: { type: 'string' } },
            buildStatus: { type: 'string' },
            notes: { type: 'string' },
          },
          required: ['fixed', 'skipped', 'buildStatus'],
        },
      }
    )
  )
)

const fixes = fixResults.filter(Boolean)
const totalFixed = fixes.reduce((acc, f) => acc + (f.fixed?.length || 0), 0)
const totalSkipped = fixes.reduce((acc, f) => acc + (f.skipped?.length || 0), 0)
log(`Fix complete: ${totalFixed} fixed, ${totalSkipped} skipped`)

return {
  totalFindings: allFindings.length,
  totalFixed,
  totalSkipped,
  findings: allFindings,
  fixes,
}
