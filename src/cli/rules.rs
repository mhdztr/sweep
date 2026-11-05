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

    let guards = crate::cli::clean::process_guards(&ctx);
    let registry = rules::registry();
    let mut results = Vec::new();
    for rule in &registry {
        let detect = rule.detect(&ctx);
        let applied = matches!(detect, rules::Detect::Ready);
        let reason = match &detect {
            rules::Detect::NotApplicable { reason } => Some(reason.clone()),
            rules::Detect::Ready => None,
        };
        let state = state_for(
            rule.tier(),
            rule.needs_sudo(),
            rule.report_only(),
            &detect,
            guards.get(rule.id()).map(String::as_str),
        );
        results.push((rule.as_ref(), applied, reason, state));
    }

    // Filter by tier if requested
    if let Some(t) = tier {
        results.retain(|(r, _, _, _)| match t {
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
            .map(|(r, applied, reason, state)| {
                json!({
                    "id": r.id(),
                    "title": r.title(),
                    "category": r.category(),
                    "tier": if r.report_only() { "report" } else { r.tier().as_str() },
                    "needs_sudo": r.needs_sudo(),
                    "applied": applied,
                    "reason": reason,
                    "report_only": r.report_only(),
                    "state": state,
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

    // Plain-text output; sanitize detected state before writing it to the terminal.
    println!("  id                  tier         sudo  default   state");
    for (r, _, _, state) in &results {
        let tier_str = if r.report_only() {
            "report"
        } else {
            r.tier().as_str()
        };
        let sudo_str = if r.needs_sudo() { "yes" } else { "-" };
        let default_str = if r.report_only() {
            "-"
        } else {
            match r.tier() {
                rules::Tier::Safe => "regular",
                rules::Tier::Caution => "off",
                rules::Tier::Destructive => "full",
            }
        };
        println!(
            "  {:<20} {:<12} {:<5} {:<9} {}",
            r.id(),
            tier_str,
            sudo_str,
            default_str,
            crate::ui::render::sanitize(state, theme.unicode)
        );
    }
    0
}

fn state_for(
    tier: rules::Tier,
    needs_sudo: bool,
    report_only: bool,
    detect: &rules::Detect,
    guard: Option<&str>,
) -> String {
    if let rules::Detect::NotApplicable { reason } = detect {
        return format!("disabled ({reason})");
    }
    if report_only {
        return "ready".into();
    }
    if tier == rules::Tier::Destructive {
        return "locked (purge gate)".into();
    }
    if let Some(reason) = guard {
        return format!("guarded ({reason})");
    }
    if tier == rules::Tier::Caution && needs_sudo {
        return "ready (sudo)".into();
    }
    "ready".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_distinguish_disabled_guarded_sudo_and_locked() {
        let ready = rules::Detect::Ready;
        assert_eq!(
            state_for(
                rules::Tier::Safe,
                false,
                false,
                &ready,
                Some("firefox running")
            ),
            "guarded (firefox running)"
        );
        assert_eq!(
            state_for(rules::Tier::Caution, true, false, &ready, None),
            "ready (sudo)"
        );
        assert_eq!(
            state_for(rules::Tier::Destructive, false, false, &ready, None),
            "locked (purge gate)"
        );
        assert_eq!(
            state_for(
                rules::Tier::Safe,
                false,
                false,
                &rules::Detect::NotApplicable {
                    reason: "disabled in config".into()
                },
                None
            ),
            "disabled (disabled in config)"
        );
    }
}
