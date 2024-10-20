use std::time::{Duration, Instant, UNIX_EPOCH};
use std::sync::{Arc};

use tokio::sync::Mutex;
use chrono::prelude::Utc;
use tokio::time::sleep;

#[derive(Clone, PartialEq)]
struct RateLimiter {
    req_window: usize,
    sec_window: Instant,
    sec_prev: Instant,
    token_window: usize,
    tokens: usize,
    rpm: usize,
    rpd: usize,
    tpm: usize,
}

impl RateLimiter {
    fn new(rpm: usize, rpd: usize, tpm: usize) -> Self {
        let now = Instant::now();
        Self {
            req_window: 0,
            sec_window: now,
            sec_prev: now,
            token_window: 0,
            tokens: 0,
            rpm,
            rpd,
            tpm,
        }
    }
    
    fn is_allowed_rpm(&mut self, now: Instant) -> bool {
        if now.duration_since(self.sec_prev).as_secs() > 60 {
            // Reset on new minute
            self.req_window = 1; 
            self.sec_prev = now; 
            return true;
        }
        if now.duration_since(self.sec_window).as_secs() > 60 {
            // Reset the second window
            self.sec_window = self.sec_prev;
            self.req_window = 1; 
        } else {
            self.req_window += 1; // Increment request count
        }
        
        self.req_window <= self.rpm
    }

    fn is_allowed_rpd(&self, _now: Instant) -> bool {
        // Implement your RPD logic here
        true // Placeholder
    }

    fn is_allowed_tpm(&mut self, now: Instant, tokens: usize, task_id: &str) -> bool {
        log::info!("{}\tToken window: {}, Tokens {}, TPM: {}, token_window+tokens: {}", task_id, self.token_window, tokens, self.tpm, self.token_window + tokens);
        if now.duration_since(self.sec_prev).as_secs() > 60 {
            log::info!("is_allowed_tpm\t{}\tnow.duration_since(sec_prev)>60\tResetting token window", task_id);
            self.token_window = tokens; // Reset tokens count
            self.sec_prev = now; 
            return true;
        }
        if now.duration_since(self.sec_window).as_secs() > 60 {
            log::info!("is_allowed_tpm\t{}\tnow.duration_since(sec_windows)>60\tResetting token window", task_id);
            self.sec_window = self.sec_prev;
            self.token_window = tokens; // Reset tokens count
        } else {
            log::info!("is_allowed_tpm\t{}\tnow.duration_since(sec_window)<60\tIncrement token window", task_id);
            self.token_window += tokens; // Increment tokens count
        }
        log::info!("is_allowed_tpm\t{}\tfn return: {}",task_id, self.token_window + tokens <= self.tpm);
        self.token_window + tokens <= self.tpm
    }
}

pub struct RateLimiterWrapper {
    pub limiter: Arc<Mutex<RateLimiter>>,
    checking_in_progress: Arc<Mutex<bool>>,
}

impl Clone for RateLimiterWrapper {
    fn clone(&self) -> Self {
        RateLimiterWrapper {
            limiter: Arc::clone(&self.limiter), 
            checking_in_progress: Arc::clone(&self.checking_in_progress),
        }
    }
}

impl RateLimiterWrapper {
    pub fn new(rpm: usize, rpd: usize, tpm: usize) -> Self {
        let limiter = RateLimiter::new(rpm, rpd, tpm);
        Self {
            limiter: Arc::new(Mutex::new(limiter)),
            checking_in_progress: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn is_allowed(&self, tokens: usize, task_id: &str ) -> bool {
        
        let mut attempt = 0;
        let max_attempts = 10;
        let mut in_progress = self.checking_in_progress.lock().await;
        while attempt < max_attempts {
            // Lock the mutex once to check all limits
            let mut limiter = self.limiter.lock().await;
            let now = Instant::now();
            log::info!("{}\t{}\tAttempt {}: Checking limits...", Utc::now().format("%Y-%m-%d %H:%M:%S.%3f").to_string(), task_id, attempt);
            let rpm_allowed = limiter.is_allowed_rpm(now);
            let rpd_allowed = limiter.is_allowed_rpd(now);
            let tpm_allowed = limiter.is_allowed_tpm(now, tokens, task_id);
            log::info!("{}\t{}\tAttempt {}: RPM allowed: {}, RPD allowed: {}, TPM allowed: {}", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S.%3f").to_string(),
                        task_id,
                        attempt,
                        rpm_allowed,
                        rpd_allowed,
                        tpm_allowed);

            if rpm_allowed && rpd_allowed && tpm_allowed {
                *in_progress = false;
                return true; // Allowed
            }

            log::info!("{}\t{}\tAttempt {}: Rate limit exceeded. RPM allowed: {}, RPD allowed: {}, TPM allowed: {}", 
                        Utc::now().format("%Y-%m-%d %H:%M:%S.%3f").to_string(),
                        task_id,
                        attempt,
                        rpm_allowed,
                        rpd_allowed,
                        tpm_allowed);

            // Release the lock before sleeping
            drop(limiter);

            // Calculate backoff duration
            let backoff_duration = std::cmp::min(Duration::from_secs(2u64.pow(attempt)), Duration::from_secs(60));
            sleep(backoff_duration).await;
            log::info!("{}\tWaiting for {:?} before retrying...", task_id, backoff_duration);

            attempt += 1;
        }
        *in_progress = false;
        log::info!("{}\tMaximum attempts reached. Exiting rate limiting checks.", task_id);
        false // Not allowed after max attempts
    }
}
