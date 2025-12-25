# VT Code Error Analytics & Monitoring Guide

## Overview

This guide provides recommendations for tracking and analyzing error patterns across LLM providers in production.

**Last Updated:** 2025-11-27T14:17:16+07:00
**Status:** Production Monitoring Framework

---

## Error Tracking Architecture

### Centralized Error Handling

All provider errors flow through centralized handlers in `error_handling.rs`:

```rust
// Error handling entry points
 handle_gemini_http_error()
 handle_anthropic_http_error()
 handle_openai_http_error()
 is_rate_limit_error()
 format_network_error()
 format_parse_error()
```

**Benefit:** Single instrumentation point for all error metrics

---

## Metrics Collection

### Recommended Metrics

#### 1. Error Rate by Provider

```rust
// Track errors per provider
counter!("llm.errors.total",
    "provider" => provider_name,
    "error_type" => error_category
);
```

**Categories:**

-   `rate_limit` - 429 errors
-   `auth_error` - 401/403 errors
-   `network_error` - Connection failures
-   `parse_error` - JSON parsing failures
-   `server_error` - 500+ errors
-   `client_error` - 400-499 errors

#### 2. Error Response Time

```rust
// Track time to detect and handle errors
histogram!("llm.error.handling_duration_ms",
    "provider" => provider_name,
    "error_type" => error_category
);
```

#### 3. Rate Limit Tracking

```rust
// Specific rate limit monitoring
counter!("llm.rate_limits.total",
    "provider" => provider_name,
    "status_code" => status.as_u16()
);

gauge!("llm.rate_limits.current",
    "provider" => provider_name
);
```

#### 4. Error Recovery Success

```rust
// Track retry success rates
counter!("llm.errors.recovered",
    "provider" => provider_name,
    "retry_count" => attempt
);

counter!("llm.errors.failed",
    "provider" => provider_name,
    "final_error" => error_type
);
```

---

## Implementation Guide

### 1. Add Metrics to Error Handlers

**Location:** `vtcode-core/src/llm/providers/error_handling.rs`

```rust
use metrics::{counter, histogram, gauge};
use std::time::Instant;

pub fn handle_gemini_http_error(
    status: StatusCode,
    body: &str,
) -> anyhow::Error {
    let start = Instant::now();

    // Categorize error
    let error_type = if is_rate_limit_error(status, body) {
        "rate_limit"
    } else if status.is_client_error() {
        "client_error"
    } else if status.is_server_error() {
        "server_error"
    } else {
        "unknown"
    };

    // Record metrics
    counter!("llm.errors.total",
        "provider" => "gemini",
        "error_type" => error_type,
        "status_code" => status.as_u16()
    );

    // Build error
    let error = if is_rate_limit_error(status, body) {
        counter!("llm.rate_limits.total", "provider" => "gemini");
        anyhow::anyhow!("Gemini API rate limit exceeded. Please try again later.")
    } else {
        // ... existing error handling
    };

    // Record handling duration
    histogram!("llm.error.handling_duration_ms",
        start.elapsed().as_millis() as f64,
        "provider" => "gemini",
        "error_type" => error_type
    );

    error
}
```

### 2. Provider-Specific Dashboards

#### Gemini Dashboard

```yaml
metrics:
    - name: "Gemini Error Rate"
      query: "rate(llm_errors_total{provider='gemini'}[5m])"

    - name: "Gemini Rate Limits"
      query: "sum(llm_rate_limits_total{provider='gemini'})"

    - name: "Gemini Error Distribution"
      query: "sum by (error_type) (llm_errors_total{provider='gemini'})"
```

#### Anthropic Dashboard

```yaml
metrics:
    - name: "Anthropic Error Rate"
      query: "rate(llm_errors_total{provider='anthropic'}[5m])"

    - name: "Anthropic Auth Errors"
      query: "sum(llm_errors_total{provider='anthropic',status_code='401'})"
```

---

## Error Pattern Analysis

### Common Error Patterns

#### 1. Rate Limit Patterns

```sql
-- Identify rate limit spikes
SELECT
    provider,
    DATE_TRUNC('hour', timestamp) as hour,
    COUNT(*) as rate_limit_count
FROM error_logs
WHERE error_type = 'rate_limit'
GROUP BY provider, hour
ORDER BY rate_limit_count DESC
LIMIT 20;
```

**Action Items:**

-   Implement exponential backoff
-   Add request queuing
-   Consider provider rotation

#### 2. Authentication Failures

```sql
-- Track auth error trends
SELECT
    provider,
    error_message,
    COUNT(*) as occurrences
FROM error_logs
WHERE status_code IN (401, 403)
GROUP BY provider, error_message
ORDER BY occurrences DESC;
```

**Action Items:**

-   Verify API key rotation
-   Check token expiration
-   Audit permission scopes

#### 3. Network Errors

```sql
-- Analyze network reliability
SELECT
    provider,
    error_type,
    AVG(retry_count) as avg_retries,
    COUNT(*) as total_errors
FROM error_logs
WHERE error_type = 'network_error'
GROUP BY provider, error_type;
```

**Action Items:**

-   Review timeout configurations
-   Check network infrastructure
-   Consider circuit breakers

---

## Alerting Rules

### Critical Alerts

#### 1. High Error Rate

```yaml
alert: HighLLMErrorRate
expr: |
    rate(llm_errors_total[5m]) > 0.1
for: 5m
labels:
    severity: critical
annotations:
    summary: "High error rate for {{ $labels.provider }}"
    description: "Error rate is {{ $value }} errors/sec"
```

#### 2. Rate Limit Threshold

```yaml
alert: RateLimitExceeded
expr: |
    sum(llm_rate_limits_total) > 100
for: 1m
labels:
    severity: warning
annotations:
    summary: "Rate limits exceeded for {{ $labels.provider }}"
    description: "{{ $value }} rate limit errors in last minute"
```

#### 3. Provider Unavailability

```yaml
alert: ProviderUnavailable
expr: |
    sum(llm_errors_total{error_type="server_error"}) > 10
for: 2m
labels:
    severity: critical
annotations:
    summary: "{{ $labels.provider }} may be unavailable"
    description: "{{ $value }} server errors in 2 minutes"
```

---

## Logging Best Practices

### Structured Logging

```rust
use tracing::{error, warn, info};

// Log with context
error!(
    provider = "gemini",
    status_code = %status,
    error_type = "rate_limit",
    "Rate limit exceeded"
);

// Log with structured data
warn!(
    provider = "anthropic",
    request_id = %request_id,
    retry_count = attempt,
    "Retrying after error"
);

// Log successful recovery
info!(
    provider = "openai",
    original_error = %original_error,
    retry_count = attempt,
    "Successfully recovered from error"
);
```

### Log Aggregation

**Recommended Stack:**

-   **Collection:** Fluentd / Vector
-   **Storage:** Elasticsearch / Loki
-   **Visualization:** Kibana / Grafana

**Query Examples:**

```
# Find all rate limit errors in last hour
provider:"gemini" AND error_type:"rate_limit" AND @timestamp:[now-1h TO now]

# Track error recovery patterns
error_type:"network_error" AND status:"recovered"

# Identify problematic requests
status_code:>=500 AND provider:"anthropic"
```

---

## Performance Monitoring

### Error Handling Performance

```rust
// Instrument error handling
let _timer = histogram!("llm.error.handling_duration_ms").start_timer();

let error = handle_gemini_http_error(status, body);

// Timer automatically records on drop
```

### Memory Tracking

```rust
// Track allocation patterns
gauge!("llm.error.allocations",
    "provider" => provider_name,
    "error_type" => error_type
);
```

---

## Error Recovery Strategies

### 1. Exponential Backoff

```rust
async fn retry_with_backoff<F, T>(
    mut operation: F,
    max_retries: u32,
) -> anyhow::Result<T>
where
    F: FnMut() -> anyhow::Result<T>,
{
    let mut attempt = 0;

    loop {
        match operation() {
            Ok(result) => {
                counter!("llm.errors.recovered",
                    "retry_count" => attempt
                );
                return Ok(result);
            }
            Err(e) if attempt < max_retries => {
                let delay = Duration::from_millis(100 * 2_u64.pow(attempt));

                warn!(
                    attempt = attempt,
                    delay_ms = delay.as_millis(),
                    error = %e,
                    "Retrying after error"
                );

                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(e) => {
                counter!("llm.errors.failed",
                    "final_error" => "max_retries_exceeded"
                );
                return Err(e);
            }
        }
    }
}
```

### 2. Circuit Breaker

```rust
struct CircuitBreaker {
    failure_threshold: u32,
    success_threshold: u32,
    timeout: Duration,
    state: Arc<Mutex<CircuitState>>,
}

impl CircuitBreaker {
    async fn call<F, T>(&self, operation: F) -> anyhow::Result<T>
    where
        F: FnOnce() -> anyhow::Result<T>,
    {
        let state = self.state.lock().await;

        match *state {
            CircuitState::Open => {
                counter!("llm.circuit_breaker.rejected");
                anyhow::bail!("Circuit breaker is open")
            }
            CircuitState::HalfOpen | CircuitState::Closed => {
                drop(state);

                match operation() {
                    Ok(result) => {
                        self.on_success().await;
                        Ok(result)
                    }
                    Err(e) => {
                        self.on_failure().await;
                        Err(e)
                    }
                }
            }
        }
    }
}
```

### 3. Provider Fallback

```rust
async fn call_with_fallback(
    primary: &dyn LLMProvider,
    fallback: &dyn LLMProvider,
    request: &LLMRequest,
) -> anyhow::Result<LLMResponse> {
    match primary.complete(request).await {
        Ok(response) => {
            counter!("llm.fallback.primary_success");
            Ok(response)
        }
        Err(e) => {
            warn!(
                primary = primary.name(),
                fallback = fallback.name(),
                error = %e,
                "Falling back to secondary provider"
            );

            counter!("llm.fallback.triggered",
                "primary" => primary.name(),
                "fallback" => fallback.name()
            );

            fallback.complete(request).await
        }
    }
}
```

---

## Dashboard Templates

### Grafana Dashboard JSON

```json
{
    "dashboard": {
        "title": "LLM Provider Errors",
        "panels": [
            {
                "title": "Error Rate by Provider",
                "targets": [
                    {
                        "expr": "sum(rate(llm_errors_total[5m])) by (provider)"
                    }
                ]
            },
            {
                "title": "Rate Limits",
                "targets": [
                    {
                        "expr": "sum(llm_rate_limits_total) by (provider)"
                    }
                ]
            },
            {
                "title": "Error Distribution",
                "targets": [
                    {
                        "expr": "sum(llm_errors_total) by (error_type)"
                    }
                ]
            },
            {
                "title": "Recovery Success Rate",
                "targets": [
                    {
                        "expr": "sum(rate(llm_errors_recovered[5m])) / sum(rate(llm_errors_total[5m]))"
                    }
                ]
            }
        ]
    }
}
```

---

## Maintenance Checklist

### Daily

-   [ ] Review error rate trends
-   [ ] Check rate limit usage
-   [ ] Verify alert health

### Weekly

-   [ ] Analyze error patterns
-   [ ] Review recovery strategies
-   [ ] Update alert thresholds

### Monthly

-   [ ] Generate error analytics report
-   [ ] Review provider reliability
-   [ ] Optimize retry strategies
-   [ ] Update documentation

---

## Conclusion

Implementing comprehensive error analytics provides:

**Proactive monitoring** - Catch issues before users
**Data-driven decisions** - Optimize based on real patterns
**Improved reliability** - Better error recovery
**Cost optimization** - Reduce unnecessary retries
**Better UX** - Faster error resolution

**Next Steps:**

1. Implement metrics collection
2. Set up dashboards
3. Configure alerts
4. Establish review cadence

---

**Document Version:** 1.0.0
**Last Updated:** 2025-11-27T14:17:16+07:00
**Status:** Ready for Implementation
