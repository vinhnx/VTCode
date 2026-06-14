use std::time::Duration;

/// Configuration for HTTP client optimization
#[derive(Clone)]
pub struct ClientConfig {
    /// Maximum number of idle connections per host
    pub pool_max_idle_per_host: usize,
    /// How long to keep idle connections alive
    pub pool_idle_timeout: Duration,
    /// TCP keepalive duration
    pub tcp_keepalive: Duration,
    /// Request timeout
    pub request_timeout: Duration,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// User agent string
    pub user_agent: String,
}

const USER_AGENT_DEFAULT: &str = "vtcode/1.0.0";
const USER_AGENT_HIGH_THROUGHPUT: &str = "vtcode/1.0.0-high-throughput";
const USER_AGENT_LOW_MEMORY: &str = "vtcode/1.0.0-low-memory";
const USER_AGENT_ULTRA_LOW_MEMORY: &str = "vtcode/1.0.0-ultra-low-memory";
const USER_AGENT_LOW_LATENCY: &str = "vtcode/1.0.0-low-latency";

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            pool_max_idle_per_host: 10,
            pool_idle_timeout: Duration::from_secs(90),
            tcp_keepalive: Duration::from_secs(60),
            request_timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(10),
            user_agent: USER_AGENT_DEFAULT.into(),
        }
    }
}

impl ClientConfig {
    /// Configuration optimized for high-throughput scenarios
    pub fn high_throughput() -> Self {
        Self {
            pool_max_idle_per_host: 20,
            pool_idle_timeout: Duration::from_secs(120),
            tcp_keepalive: Duration::from_secs(60),
            request_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
            user_agent: USER_AGENT_HIGH_THROUGHPUT.into(),
        }
    }

    /// Configuration optimized for low memory usage (< 100MB target)
    pub fn low_memory() -> Self {
        Self {
            pool_max_idle_per_host: 3,
            pool_idle_timeout: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(30),
            request_timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(5),
            user_agent: USER_AGENT_LOW_MEMORY.into(),
        }
    }

    /// Configuration optimized for ultra-low memory (< 50MB target)
    pub fn ultra_low_memory() -> Self {
        Self {
            pool_max_idle_per_host: 1,
            pool_idle_timeout: Duration::from_secs(10),
            tcp_keepalive: Duration::from_secs(15),
            request_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(3),
            user_agent: USER_AGENT_ULTRA_LOW_MEMORY.into(),
        }
    }

    /// Configuration optimized for low-latency scenarios
    pub fn low_latency() -> Self {
        Self {
            pool_max_idle_per_host: 5,
            pool_idle_timeout: Duration::from_secs(30),
            tcp_keepalive: Duration::from_secs(30),
            request_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            user_agent: USER_AGENT_LOW_LATENCY.into(),
        }
    }
}
