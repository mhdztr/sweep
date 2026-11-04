//! `sweep rules list`. Spec: `docs/CLI-UX.md` §2.6.

use crate::cli::{Root, TierArg};
use crate::ctx::Ctx;
use crate::rules;
use crate::ui::Theme;

pub fn run(_root: Root, tier: Option<TierArg>, json: bool) -> i32 {
    let ctx = match Ctx::build(&_root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let theme = Theme {
        color: ctx.flags.color,
        unicode: ctx.flags.unicode,
    };

    let registry = rules::registry();
    let mut results = Vec::new();
    for rule in &registry {
        // For rules list, we show state based on detect
        let detect = rule.detect(&ctx);
        let applied = matches!(detect, rules::Detect::Ready);
        let reason = match &detect {
            rules::Detect::NotApplicable { reason } => Some(reason.clone()),
            rules::Detect::Ready => None,
        };
        results.push((rule.as_ref(), applied, reason));
    }

    // Filter by tier if requested
    if let Some(t) = tier {
        results.retain(|(r, _, _)| match t {
            TierArg::Safe => matches!(r.tier(), rules::Tier::Safe),
            TierArg::Caution => matches!(r.tier(), rules::Tier::Caution),
            TierArg::Destructive => matches!(r.tier(), rules::Tier::Destructive),
            TierArg::Report => r.report_only(),
        });
    }

    if json {
        // For M3, minimal JSON
        use serde_json::json;
        let items: Vec<_> = results
            .iter()
            .map(|(r, applied, reason)| {
                json!({
                    "id": r.id(),
                    "title": r.title(),
                    "category": r.category(),
                    "tier": format!("{:?}", r.tier()).to_lowercase(),
                    "needs_sudo": r.needs_sudo(),
                    "applied": applied,
                    "reason": reason,
                    "report_only": r.report_only(),
                })
            })
            .collect();
        let out = std::io::stdout();
        if serde_json::to_writer(out.lock(), &json!({"rules": items})).is_err() {
            eprintln!("error: failed to write JSON");
            return 1;
        }
        println!();
        return 0;
    }

    // ASCII-clean output
    println!("  id                  tier         sudo  default   state");
    for (r, applied, reason) in &results {
        if r.report_only() {
            let tier_str = "report";
            let sudo_str = if r.needs_sudo() { "yes" } else { "-" };
            let default_str = "-";
            let state_str = if *applied {
                if let Some(reas) = reason {
                    format!("ready ({})", reas)
                } else {
                    "ready".to_string()
                }
            } else if let Some(reas) = reason {
                format!("disabled ({})", reas)
            } else {
                "ready".to_string()
            };
            println!(
                "  {:<20} {:<12} {:<5} {:<9} {}",
                r.id(),
                tier_str,
                sudo_str,
                default_str,
                state_str
            );
            continue;
        }
        let tier_str = match r.tier() {
            rules::Tier::Safe => "safe",
            rules::Tier::Caution => "caution",
            rules::Tier::Destructive => "destructive",
        };
        let sudo_str = if r.needs_sudo() { "yes" } else { "-" };
        let default_str = match r.tier() {
            rules::Tier::Safe => "regular",
            rules::Tier::Caution => "off",
            rules::Tier::Destructive => "full",
        };
        let state_str = if !*applied {
            if let Some(reas) = reason {
                format!("disabled ({})", reas)
            } else {
                "disabled".to_string()
            }
        } else if r.tier() == rules::Tier::Destructive {
            "locked (purge gate)".to_string()
        } else if r.tier() == rules::Tier::Caution {
            if r.needs_sudo() {
                "ready (sudo)".to_string()
            } else {
                "ready".to_string()
            }
        } else {
            "ready".to_string()
        };
        let _ = theme;
        println!(
            "  {:<20} {:<12} {:<5} {:<9} {}",
            r.id(),
            tier_str,
            sudo_str,
            default_str,
            state_str
        );
    }
    0
}
