//! 输入总线：跨平台输入事件协议与「触摸 → 虚拟指针/视角」映射状态机。
//!
//! 这是五端 UI/壳层与游戏表面之间唯一的输入契约：
//! - 各端原生输入（触摸/手柄/键鼠）统一翻译成 `InputEvent` 流；
//! - `VirtualCursor` 负责触摸屏上的指针模拟与 grab 模式下的视角拖动；
//! - 事件以 serde 序列化，方便 FFI 侧用 JSON 走桥（后续替换为 ABI 结构体）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchPhase {
    Begin,
    Move,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    Touch {
        phase: TouchPhase,
        id: u32,
        x: f32,
        y: f32,
    },
    PointerMove {
        x: f32,
        y: f32,
    },
    PointerButton {
        button: PointerButton,
        down: bool,
    },
    /// grab 模式下的视角拖动增量（像素）。
    LookDelta {
        dx: f32,
        dy: f32,
    },
    Wheel {
        dx: f32,
        dy: f32,
    },
    Key {
        code: u32,
        down: bool,
    },
    Text {
        chars: String,
    },
    GamepadAxis {
        axis: u8,
        value: f32,
    },
    GamepadButton {
        button: u8,
        down: bool,
    },
}

/// 触摸 → 指针/视角映射状态机。
///
/// - 未 grab：单指点击 = 移动 + 左键按下/抬起；拖动 = 指针移动。
/// - grab（游戏捕获鼠标）：触摸位移转 `LookDelta`，双指长按转右键由壳层处理。
#[derive(Debug, Clone)]
pub struct VirtualCursor {
    pub pos: (f32, f32),
    pub pressed: bool,
    pub grabbed: bool,
    /// 视角拖动灵敏度（grab 模式下 dx/dy 的放大系数）。
    pub look_sensitivity: f32,
}

impl Default for VirtualCursor {
    fn default() -> Self {
        Self {
            pos: (0.0, 0.0),
            pressed: false,
            grabbed: false,
            look_sensitivity: 1.0,
        }
    }
}

impl VirtualCursor {
    /// 处理一次触摸事件，产出映射后的输入事件序列。
    pub fn on_touch(&mut self, phase: TouchPhase, x: f32, y: f32) -> Vec<InputEvent> {
        match phase {
            TouchPhase::Begin => {
                self.pos = (x, y);
                if self.grabbed {
                    Vec::new() // grab 下位移按 Move 阶段累计
                } else {
                    self.pressed = true;
                    vec![
                        InputEvent::PointerMove { x, y },
                        InputEvent::PointerButton {
                            button: PointerButton::Left,
                            down: true,
                        },
                    ]
                }
            }
            TouchPhase::Move => {
                if self.grabbed {
                    let (px, py) = self.pos;
                    let dx = (x - px) * self.look_sensitivity;
                    let dy = (y - py) * self.look_sensitivity;
                    self.pos = (x, y);
                    if dx != 0.0 || dy != 0.0 {
                        vec![InputEvent::LookDelta { dx, dy }]
                    } else {
                        Vec::new()
                    }
                } else {
                    self.pos = (x, y);
                    vec![InputEvent::PointerMove { x, y }]
                }
            }
            TouchPhase::End => {
                if self.grabbed {
                    Vec::new()
                } else if self.pressed {
                    self.pressed = false;
                    vec![InputEvent::PointerButton {
                        button: PointerButton::Left,
                        down: false,
                    }]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// 切换 grab 模式（游戏捕获/释放鼠标时由壳层调用）。
    /// 若仍有按下的指针，补一次左键抬起，防止游戏侧卡键。
    pub fn set_grabbed(&mut self, grabbed: bool) -> Vec<InputEvent> {
        let mut out = Vec::new();
        if self.pressed {
            self.pressed = false;
            out.push(InputEvent::PointerButton {
                button: PointerButton::Left,
                down: false,
            });
        }
        self.grabbed = grabbed;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(events: &[InputEvent]) -> Vec<&'static str> {
        events
            .iter()
            .map(|e| match e {
                InputEvent::PointerMove { .. } => "move",
                InputEvent::PointerButton { down: true, .. } => "press",
                InputEvent::PointerButton { down: false, .. } => "release",
                InputEvent::LookDelta { .. } => "look",
                _ => "other",
            })
            .collect()
    }

    #[test]
    fn tap_maps_to_press_release() {
        let mut cur = VirtualCursor::default();
        let ev = cur.on_touch(TouchPhase::Begin, 100.0, 200.0);
        assert_eq!(kinds(&ev), vec!["move", "press"]);
        let ev = cur.on_touch(TouchPhase::End, 100.0, 200.0);
        assert_eq!(kinds(&ev), vec!["release"]);
    }

    #[test]
    fn drag_maps_to_move_sequence() {
        let mut cur = VirtualCursor::default();
        let _ = cur.on_touch(TouchPhase::Begin, 0.0, 0.0);
        let ev = cur.on_touch(TouchPhase::Move, 30.0, 40.0);
        assert_eq!(kinds(&ev), vec!["move"]);
        assert_eq!(cur.pos, (30.0, 40.0));
        let ev = cur.on_touch(TouchPhase::End, 30.0, 40.0);
        assert_eq!(kinds(&ev), vec!["release"]);
    }

    #[test]
    fn grab_mode_emits_look_delta() {
        let mut cur = VirtualCursor::default();
        assert!(cur.set_grabbed(true).is_empty());
        let _ = cur.on_touch(TouchPhase::Begin, 0.0, 0.0);
        let ev = cur.on_touch(TouchPhase::Move, 10.0, -5.0);
        assert_eq!(kinds(&ev), vec!["look"]);
        // 灵敏度生效
        cur.look_sensitivity = 2.0;
        let ev = cur.on_touch(TouchPhase::Move, 20.0, -15.0);
        if let InputEvent::LookDelta { dx, dy } = ev[0] {
            assert_eq!((dx, dy), (20.0, -20.0));
        } else {
            panic!("expected LookDelta");
        }
    }

    #[test]
    fn leaving_grab_releases_stuck_button() {
        let mut cur = VirtualCursor::default();
        let _ = cur.on_touch(TouchPhase::Begin, 5.0, 5.0); // pressed
        let ev = cur.set_grabbed(true);
        // 切入 grab 时压住的左键被释放，避免游戏侧卡键
        assert_eq!(kinds(&ev), vec!["release"]);
    }

    #[test]
    fn events_roundtrip_json() {
        let e = InputEvent::Touch {
            phase: TouchPhase::Begin,
            id: 1,
            x: 1.5,
            y: 2.5,
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: InputEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(
            s,
            r#"{"type":"touch","phase":"begin","id":1,"x":1.5,"y":2.5}"#
        );
        assert!(matches!(back, InputEvent::Touch { .. }));
    }
}
