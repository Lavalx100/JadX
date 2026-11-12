use crate::types::{
    MacDetails, MemoryCandidate, MemoryReport, ModelInfo, ProcessResourceUsage, ResourcePolicy,
    ResourceTelemetry,
};
use chrono::Local;
use std::process::Command;

pub fn total_memory_bytes() -> u64 {
    Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

pub fn memory_pressure() -> (String, Option<u64>) {
    let total = total_memory_bytes();
    let output = Command::new("memory_pressure").output().ok();
    let text = output
        .and_then(|value| String::from_utf8(value.stdout).ok())
        .unwrap_or_default();
    let free_percent = text.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if lower.contains("free percentage") {
            lower
                .split('%')
                .next()
                .and_then(|prefix| prefix.rsplit_once(' ').map(|(_, number)| number.trim()))
                .and_then(|number| number.parse::<u64>().ok())
        } else {
            None
        }
    });
    let available = free_percent.map(|percent| total.saturating_mul(percent) / 100);
    let pressure = match free_percent.unwrap_or(50) {
        0..=12 => "high",
        13..=28 => "elevated",
        _ => "normal",
    };
    (pressure.to_string(), available)
}

pub fn memory_report(model: Option<&ModelInfo>) -> MemoryReport {
    let total = total_memory_bytes();
    let (pressure, available) = memory_pressure();
    let model_bytes = model
        .and_then(|model| model.size_bytes)
        .unwrap_or(16 * 1024 * 1024 * 1024);
    let maximum = model
        .and_then(|model| model.max_context_length)
        .unwrap_or(262_144);
    let candidates = [8_192_u64, 12_288, 16_384, 24_576, 32_768, 49_152, 65_536]
        .into_iter()
        .filter(|candidate| *candidate <= maximum)
        .map(|context_length| {
            let estimated_bytes = model_bytes.saturating_add(context_length.saturating_mul(256 * 1024));
            let headroom = 6 * 1024 * 1024 * 1024;
            let accepted = total > estimated_bytes.saturating_add(headroom)
                && available.unwrap_or(total) > headroom;
            MemoryCandidate {
                context_length,
                accepted,
                estimated_bytes: Some(estimated_bytes),
                rationale: if accepted {
                    "Heuristic estimate leaves normal-mode system headroom; verify with LM Studio and benchmark.".to_string()
                } else {
                    "Rejected by conservative memory headroom heuristic; verify after reducing other memory pressure.".to_string()
                },
            }
        })
        .collect::<Vec<_>>();
    let available_for_tuning = available.unwrap_or(total);
    let conservative_cap = if model_bytes >= 12 * 1024 * 1024 * 1024 {
        16_384
    } else if available_for_tuning <= 12 * 1024 * 1024 * 1024 {
        8_192
    } else if available_for_tuning <= 16 * 1024 * 1024 * 1024 {
        16_384
    } else {
        32_768
    }
    .min(maximum);
    let recommended_context = candidates
        .iter()
        .filter(|candidate| candidate.accepted && candidate.context_length <= conservative_cap)
        .map(|candidate| candidate.context_length)
        .max()
        .unwrap_or(8_192.min(maximum));
    let selected_context = model
        .and_then(|value| value.loaded_instances.first())
        .and_then(|instance| instance.context_length);
    MemoryReport {
        total_bytes: total,
        available_bytes: available,
        pressure,
        recommended_context,
        candidates,
        selected_context,
    }
}

pub fn select_model(models: &[ModelInfo]) -> Option<ModelInfo> {
    models.iter().cloned().max_by_key(|model| {
        let haystack = format!(
            "{} {} {} {}",
            model.key,
            model.display_name.as_deref().unwrap_or_default(),
            model.architecture.as_deref().unwrap_or_default(),
            model.compatibility_type.as_deref().unwrap_or_default()
        )
        .to_ascii_lowercase();
        let mut score = 0_i32;
        if model.loaded_instances.len() > 0 || model.state.as_deref() == Some("loaded") {
            score += 100;
        }
        for token in ["qwen", "27b", "mlx", "uncensored"] {
            if haystack.contains(token) {
                score += 20;
            }
        }
        if model
            .quantization
            .as_ref()
            .and_then(|quantization| quantization.name.as_deref())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("4")
        {
            score += 5;
        }
        score
    })
}

pub fn local_system_summary() -> serde_json::Value {
    let (pressure, available) = memory_pressure();
    serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpu": Command::new("sysctl").args(["-n", "machdep.cpu.brand_string"]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()),
        "total_memory_bytes": total_memory_bytes(),
        "available_memory_bytes": available,
        "memory_pressure": pressure,
        "local_time": Local::now().to_rfc3339(),
    })
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
}

fn command_text_any(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                Some(stdout)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                (!stderr.is_empty()).then_some(stderr)
            }
        })
}

fn first_labeled_line(text: &str, label: &str) -> Option<String> {
    text.lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix(label)
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn parse_uptime_seconds() -> Option<u64> {
    let boot = command_text("sysctl", &["-n", "kern.boottime"])?;
    let seconds = boot
        .split("sec =")
        .nth(1)?
        .split(',')
        .next()?
        .trim()
        .parse::<i64>()
        .ok()?;
    let now = chrono::Utc::now().timestamp();
    (now > seconds).then_some((now - seconds) as u64)
}

fn parse_battery() -> (Option<f64>, Option<bool>) {
    let value = command_text_any("pmset", &["-g", "batt"]).unwrap_or_default();
    let percent = value
        .split('%')
        .next()
        .and_then(|prefix| {
            prefix
                .rsplit(|character: char| !character.is_ascii_digit() && character != '.')
                .next()
        })
        .and_then(|number| number.parse::<f64>().ok());
    let lower = value.to_ascii_lowercase();
    let charging = if lower.contains("no battery") {
        None
    } else if lower.contains("charging") || lower.contains("charged") || lower.contains("ac power")
    {
        Some(true)
    } else if lower.contains("discharging") {
        Some(false)
    } else {
        None
    };
    (percent, charging)
}

pub fn mac_details() -> MacDetails {
    let product_name = command_text("sw_vers", &["-productName"]);
    let os_version = command_text("sw_vers", &["-productVersion"]);
    let model_identifier = command_text("sysctl", &["-n", "hw.model"]);
    let chip = command_text("sysctl", &["-n", "machdep.cpu.brand_string"]);
    let hardware = if model_identifier.is_none() || chip.is_none() {
        command_text_any(
            "system_profiler",
            &["SPHardwareDataType", "-detailLevel", "mini"],
        )
        .unwrap_or_default()
    } else {
        String::new()
    };
    let model_identifier =
        model_identifier.or_else(|| first_labeled_line(&hardware, "Model Identifier:"));
    let chip = chip.or_else(|| first_labeled_line(&hardware, "Chip:"));
    let (battery_percent, battery_charging) = parse_battery();
    MacDetails {
        product_name,
        model_identifier,
        chip,
        os_version,
        architecture: std::env::consts::ARCH.to_string(),
        total_memory_bytes: total_memory_bytes(),
        uptime_seconds: parse_uptime_seconds(),
        battery_percent,
        battery_charging,
    }
}

fn swap_used_bytes() -> Option<u64> {
    let value = command_text("sysctl", &["-n", "vm.swapusage"])?;
    let used = value
        .split_whitespace()
        .skip_while(|token| *token != "used")
        .nth(2)?;
    let number = used
        .trim_end_matches('M')
        .trim_end_matches('G')
        .parse::<f64>()
        .ok()?;
    if used.ends_with('G') {
        Some((number * 1024.0 * 1024.0 * 1024.0) as u64)
    } else {
        Some((number * 1024.0 * 1024.0) as u64)
    }
}

fn process_usage() -> (Option<f64>, Option<u64>) {
    let pid = std::process::id().to_string();
    let value = command_text("ps", &["-o", "%cpu=", "-o", "rss=", "-p", &pid]).unwrap_or_default();
    let mut fields = value.split_whitespace();
    let cpu = fields.next().and_then(|value| value.parse::<f64>().ok());
    let rss_kb = fields.next().and_then(|value| value.parse::<u64>().ok());
    (cpu, rss_kb.map(|value| value * 1024))
}

fn process_resource_table() -> Vec<ProcessResourceUsage> {
    let output = match Command::new("ps")
        .args(["-axo", "pid=,comm=,%cpu=,rss="])
        .output()
    {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };
    let own_pid = std::process::id();
    let mut processes = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 4 {
                return None;
            }
            let pid = fields.first()?.parse::<u32>().ok()?;
            let cpu = fields
                .get(fields.len().saturating_sub(2))?
                .parse::<f64>()
                .ok()?;
            let rss_kb = fields.last()?.parse::<u64>().ok()?;
            let name = fields[1..fields.len() - 2].join(" ");
            let lower = name.to_ascii_lowercase();
            let is_provider = lower.contains("lm studio")
                || lower.contains("lm-studio")
                || lower.contains("llmster");
            Some(ProcessResourceUsage {
                name: if pid == own_pid { "JadX".into() } else { name },
                cpu_percent: cpu,
                memory_bytes: rss_kb.saturating_mul(1024),
                is_harness: pid == own_pid,
                is_provider,
            })
        })
        .collect::<Vec<_>>();
    processes.sort_by(|left, right| right.memory_bytes.cmp(&left.memory_bytes));
    processes
}

fn system_cpu_usage() -> Option<f64> {
    let value = command_text("top", &["-l", "1", "-n", "0"])?;
    let line = value
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("cpu usage:"))?;
    let user = line
        .split("user,")
        .next()?
        .rsplit_once(':')?
        .1
        .trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()?;
    let system = line
        .split("% sys")
        .next()?
        .rsplit_once(',')?
        .1
        .trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()?;
    Some(user + system)
}

fn load_average() -> Option<f64> {
    let value = command_text("sysctl", &["-n", "vm.loadavg"])?;
    value
        .trim_matches(|character| {
            character == '{' || character == '}' || character == '(' || character == ')'
        })
        .split_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
}

fn thermal_state() -> String {
    let thermlog = command_text("pmset", &["-g", "therm"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    if thermlog.contains("critical") || thermlog.contains("heavy") {
        return "critical".into();
    }
    if thermlog.contains("moderate") || thermlog.contains("thrott") {
        return "elevated".into();
    }
    if thermlog.is_empty() {
        "unknown".into()
    } else {
        "normal".into()
    }
}

pub fn resource_telemetry(policy: ResourcePolicy) -> ResourceTelemetry {
    let (memory_pressure, available_memory_bytes) = memory_pressure();
    let (process_cpu_percent, process_memory_bytes) = process_usage();
    let all_processes = process_resource_table();
    let provider_processes = all_processes
        .iter()
        .filter(|process| process.is_provider)
        .collect::<Vec<_>>();
    let provider_cpu_percent = (!provider_processes.is_empty()).then_some(
        provider_processes
            .iter()
            .map(|process| process.cpu_percent)
            .sum(),
    );
    let provider_memory_bytes = (!provider_processes.is_empty()).then_some(
        provider_processes
            .iter()
            .map(|process| process.memory_bytes)
            .sum(),
    );
    let provider_process_count = provider_processes.len() as u32;
    let top_processes = all_processes.into_iter().take(8).collect::<Vec<_>>();
    let headroom_target_bytes = match policy {
        ResourcePolicy::Adaptive => 6 * 1024 * 1024 * 1024,
        ResourcePolicy::Performance => 4 * 1024 * 1024 * 1024,
        ResourcePolicy::Cool => 8 * 1024 * 1024 * 1024,
    };
    ResourceTelemetry {
        total_memory_bytes: total_memory_bytes(),
        available_memory_bytes,
        memory_pressure,
        swap_used_bytes: swap_used_bytes(),
        process_cpu_percent,
        system_cpu_percent: system_cpu_usage(),
        process_memory_bytes,
        provider_cpu_percent,
        provider_memory_bytes,
        provider_processes: provider_process_count,
        top_processes,
        thermal_state: thermal_state(),
        load_average: load_average(),
        headroom_target_bytes,
        emergency_floor_bytes: 4 * 1024 * 1024 * 1024,
        captured_at: Local::now().to_rfc3339(),
    }
}

pub fn resource_decision(telemetry: &ResourceTelemetry) -> Result<(), String> {
    if telemetry.memory_pressure == "high" {
        return Err("Inference paused because macOS reports high memory pressure.".into());
    }
    if telemetry
        .available_memory_bytes
        .unwrap_or(telemetry.total_memory_bytes)
        < telemetry.emergency_floor_bytes
    {
        return Err("Inference paused to preserve emergency memory headroom.".into());
    }
    if telemetry.thermal_state == "critical" {
        return Err("Inference paused because thermal pressure is critical.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_selection_prefers_loaded_qwen_candidate() {
        let models = vec![
            ModelInfo {
                key: "generic/7b".into(),
                ..Default::default()
            },
            ModelInfo {
                key: "orcarouter/qwen3.8-27b-uncensored-mlx".into(),
                loaded_instances: vec![Default::default()],
                ..Default::default()
            },
        ];
        assert_eq!(
            select_model(&models).unwrap().key,
            "orcarouter/qwen3.8-27b-uncensored-mlx"
        );
    }

    #[test]
    fn resource_policy_has_conservative_targets() {
        let adaptive = resource_telemetry(ResourcePolicy::Adaptive);
        let cool = resource_telemetry(ResourcePolicy::Cool);
        assert!(cool.headroom_target_bytes >= adaptive.headroom_target_bytes);
        assert_eq!(adaptive.emergency_floor_bytes, 4 * 1024 * 1024 * 1024);
    }
}
