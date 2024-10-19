use std::time::{Instant,Duration};
use std::sync::{Arc, MutexGuard};
use tokio::sync::Mutex;
use chrono::prelude::Utc;

use tokio::time::sleep;

#[derive(Clone,PartialEq)]
struct RateLimiter {
    now: Instant,
    req_window: usize,
    sec_window: Instant,
    sec_prev: Instant,
    rpm: usize,
    rpd: usize,
    tpm: usize,
}

impl RateLimiter {
    fn new(rpm: usize, rpd: usize, tpm: usize) -> Self {
        let now = Instant::now();
        Self {
            now,
            req_window: 0,
            sec_window: now,
            sec_prev: now,
            rpm,
            rpd,
            tpm,
        }
    }
    
    fn is_allowed_rpm(&mut self, now: Instant) -> bool {
        let print_time  = Utc::now();
        let formatted_time = print_time.format("%Y-%m-%d %H:%M:%S").to_string();
        
        
        log::info!("\nnow\t\t\treq_w\tsec_w\t\tsec_prev\trpm\trpd\ttmp\n{}\t{}\t{}\t{}\t{}\t{}\t{}\n", formatted_time, self.req_window, now.duration_since(self.sec_window).as_secs_f32(), now.duration_since(self.sec_prev).as_secs_f32(), self.rpm, self.rpd, self.tpm);
        if  now.duration_since(self.sec_prev).as_secs() > 60 {
            self.req_window = 1; // Reset request count
            self.sec_prev = now; // Update last request time
            return true;
        }
        if now.duration_since(self.sec_window).as_secs() > 60 {
            self.sec_window = self.sec_prev;
            self.req_window = 1; // Reset request count
        } else {
            self.req_window += 1; // Increment request count
        }
        
        self.sec_prev = now; // Update last request time
        
        if self.req_window > self.rpm  {
            return false;
        } else {
            self.req_window += 1; // Increment request count
            return true;
        }
    }


    fn is_allowed_rpd(&self, now:Instant) -> bool {
        // Implement your RPD logic here
        true // Placeholder
    }

    fn is_allowed_tpm(&self, now:Instant) -> bool {
        // Implement your TPM logic here
        true // Placeholder
    }
}


pub struct RateLimiterWrapper {
    pub limiter: Arc<Mutex<RateLimiter>>,
}
impl Clone for RateLimiterWrapper {
    fn clone(&self) -> Self {
        RateLimiterWrapper {
            limiter: Arc::clone(&self.limiter), // Clone the Arc
        }
    }
}
impl PartialEq for RateLimiterWrapper {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.limiter, &other.limiter)
    }
}

impl RateLimiterWrapper {
    pub fn new(rpm: usize, rpd: usize, tpm: usize) -> Self {
        let limiter = RateLimiter::new(rpm, rpd, tpm);
        Self {
            limiter: Arc::new(Mutex::new(limiter)),
        }
    }

    pub async fn is_allowed(&self) -> bool {
        let mut limiter = self.limiter.lock().await;
        let now = Instant::now();       
        let mut attempt = 0; // Attempt counter for backoff
        
        while !limiter.is_allowed_rpm(now) || !limiter.is_allowed_rpd(now) || !limiter.is_allowed_tpm(now) {
            // Calculate the backoff duration (2^attempt seconds)
            let backoff_duration = Duration::from_secs(2u64.pow(attempt));
            sleep(backoff_duration).await; // Wait asynchronously
            
            // Optional: You may want to log the backoff attempt
            log::info!("Rate limit exceeded, waiting for {:?} before retrying...", backoff_duration);

            attempt += 1; // Increase the attempt count

            // Re-check the limits
            limiter = self.limiter.lock().await; // Re-lock the mutex for the next iteration
        }

        
        return true // Allow request
    }

    
   
}
