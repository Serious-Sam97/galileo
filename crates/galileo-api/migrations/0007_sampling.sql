-- Tail-based sampling policy per project. rate 1.0 = keep everything (no buffering).
ALTER TABLE project_settings
    ADD COLUMN IF NOT EXISTS sampling JSONB NOT NULL DEFAULT '{"rate": 1.0, "keep_errors": true, "slow_ms": 2000, "keep_llm": true, "decision_delay_secs": 10}';
