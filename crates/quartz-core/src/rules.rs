//! 规则引擎：Mojang 版本 JSON 中 `rules` 字段的求值。
//!
//! 语义（与主流社区启动器一致）：
//! - `rules` 为空 -> 允许；
//! - 存在 `rules` 时按顺序求值，**最后一个匹配的规则决定结果**；
//! - 没有任何规则匹配 -> 不允许。

use crate::model::{OsRule, Rule, RuleAction};
use std::collections::BTreeMap;

/// 规则求值上下文：目标 OS / 架构 / 特性开关。
#[derive(Debug, Clone, Default)]
pub struct RuntimeCtx {
    /// 小写：linux / windows / osx / android / ios / harmonyos
    pub os: String,
    /// 小写：x86_64 / x86 / arm64 / arm
    pub arch: String,
    /// 特性开关，如 is_demo_user=true、has_custom_resolution=true。
    pub features: BTreeMap<String, bool>,
}

impl RuntimeCtx {
    pub fn new(os: &str, arch: &str) -> Self {
        Self {
            os: os.to_ascii_lowercase(),
            arch: arch.to_ascii_lowercase(),
            features: BTreeMap::new(),
        }
    }

    pub fn with_feature(mut self, key: &str, value: bool) -> Self {
        self.features.insert(key.to_string(), value);
        self
    }
}

fn os_rule_matches(rule: &OsRule, ctx: &RuntimeCtx) -> bool {
    if let Some(name) = &rule.name {
        if !name.eq_ignore_ascii_case(&ctx.os) {
            return false;
        }
    }
    if let Some(arch) = &rule.arch {
        if !arch.eq_ignore_ascii_case(&ctx.arch) {
            return false;
        }
    }
    true
}

fn feature_value_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        // 对象形式的特性（如 has_custom_resolution 的附加参数）视为开启。
        serde_json::Value::Object(_) => true,
        _ => true,
    }
}

fn rule_matches(rule: &Rule, ctx: &RuntimeCtx) -> bool {
    if let Some(os) = &rule.os {
        if !os_rule_matches(os, ctx) {
            return false;
        }
    }
    if let Some(features) = &rule.features {
        for (key, want) in features {
            let enabled = ctx.features.get(key).copied().unwrap_or(false);
            if enabled != feature_value_truthy(want) {
                return false;
            }
        }
    }
    true
}

/// 对一组规则求值。见模块级文档的语义说明。
pub fn evaluate(rules: &[Rule], ctx: &RuntimeCtx) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, ctx) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(os_name: &str, action: RuleAction) -> Rule {
        serde_json::from_value(json!({ "action": action, "os": { "name": os_name } })).unwrap()
    }

    #[test]
    fn empty_rules_allow() {
        assert!(evaluate(&[], &RuntimeCtx::new("linux", "arm64")));
    }

    #[test]
    fn allow_osx_denies_windows() {
        let rules = vec![rule("osx", RuleAction::Allow)];
        assert!(evaluate(&rules, &RuntimeCtx::new("osx", "arm64")));
        assert!(!evaluate(&rules, &RuntimeCtx::new("windows", "x86_64")));
        assert!(!evaluate(&rules, &RuntimeCtx::new("android", "arm64")));
    }

    #[test]
    fn last_matching_rule_wins() {
        // 先 allow linux，再 disallow linux => 最终禁止（最后一条匹配规则决定）。
        let rules = vec![
            rule("linux", RuleAction::Allow),
            rule("linux", RuleAction::Disallow),
        ];
        assert!(!evaluate(&rules, &RuntimeCtx::new("linux", "x86_64")));
        // 未匹配任何规则的上下文 => 禁止。
        assert!(!evaluate(&rules, &RuntimeCtx::new("osx", "arm64")));
    }

    #[test]
    fn arch_rule_respected() {
        let rules: Vec<Rule> = serde_json::from_value(json!([
            { "action": "allow", "os": { "name": "windows", "arch": "arm64" } }
        ]))
        .unwrap();
        assert!(evaluate(&rules, &RuntimeCtx::new("windows", "arm64")));
        assert!(!evaluate(&rules, &RuntimeCtx::new("windows", "x86_64")));
    }

    #[test]
    fn feature_rules() {
        let demo: Vec<Rule> = serde_json::from_value(json!([
            { "action": "allow", "features": { "is_demo_user": true } }
        ]))
        .unwrap();
        assert!(!evaluate(&demo, &RuntimeCtx::new("linux", "x86_64")));
        assert!(evaluate(
            &demo,
            &RuntimeCtx::new("linux", "x86_64").with_feature("is_demo_user", true)
        ));
    }
}
