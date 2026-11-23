package com.quartz.launcher

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/**
 * 游戏内虚拟控制层（v0.1.5）：
 * - 左下方向键（W/A/S/D）+ 跳跃（Space）+ 潜行（Shift）；
 * - 右下「按住攻击」开关（挖掘/攻击的持续左键）；
 * - 顶部右侧：Esc（暂停）、E（背包）、T（聊天）、键盘开关（软键盘输入聊天）。
 * 全部按键经 GameIpc（Messenger IPC）-> :game 进程桥 -> LWJGL 键回调进入游戏。
 */
class GameControls(context: Context) : FrameLayout(context) {

    private val hiddenInput: EditText
    private var holdLeft = false

    init {
        // ---- 隐藏输入框（软键盘文本 -> sendChar）----
        hiddenInput = EditText(context).apply {
            layoutParams = LayoutParams(1, 1)
            alpha = 0f
            isCursorVisible = false
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun afterTextChanged(s: Editable?) {
                    val text = s?.toString() ?: return
                    if (text.isNotEmpty()) {
                        for (ch in text) {
                            GameIpc.char(ch.code, 0)
                        }
                        setText("")
                    }
                }
            })
        }
        addView(hiddenInput)

        // ---- 顶部右侧功能键 ----
        val topRow = row().apply {
            gravity = Gravity.TOP or Gravity.END
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
            setPadding(0, dp(10), dp(10), 0)
        }
        topRow.addView(button("Esc", KeyEvent.KEYCODE_ESCAPE))
        topRow.addView(button("E", KeyEvent.KEYCODE_E))
        topRow.addView(button("T", KeyEvent.KEYCODE_T))
        topRow.addView(imeButton())
        addView(topRow)

        // ---- 左下方向键 ----
        val dpad = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.START
            )
            setPadding(dp(12), 0, 0, dp(14))
        }
        dpad.addView(row().apply { addView(button("W", KeyEvent.KEYCODE_W)) })
        dpad.addView(row().apply {
            addView(button("A", KeyEvent.KEYCODE_A))
            addView(button("S", KeyEvent.KEYCODE_S))
            addView(button("D", KeyEvent.KEYCODE_D))
        })
        addView(dpad)

        // ---- 右下动作键 ----
        val actions = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.END
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.END
            )
            setPadding(0, 0, dp(12), dp(14))
        }
        actions.addView(button("跳跃", KeyEvent.KEYCODE_SPACE, wide = true))
        actions.addView(button("潜行", KeyEvent.KEYCODE_SHIFT_LEFT, wide = true))
        actions.addView(holdButton())
        addView(actions)
    }

    private fun row(): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
    }

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()

    private fun keyStyle(): GradientDrawable = GradientDrawable().apply {
        setColor(0xB3141A26.toInt())
        cornerRadius = dp(14).toFloat()
        setStroke(dp(1), 0x66628895.toInt())
    }

    private fun baseButton(label: String, wide: Boolean = false): TextView {
        return TextView(context).apply {
            text = label
            gravity = Gravity.CENTER
            textSize = 13f
            setTextColor(0xFFE8EBF1.toInt())
            typeface = Typeface.DEFAULT_BOLD
            background = keyStyle()
            val w = if (wide) dp(104) else dp(58)
            layoutParams = LinearLayout.LayoutParams(w, dp(52)).apply {
                setMargins(dp(5), dp(5), dp(5), dp(5))
            }
        }
    }

    /** 按下/抬起键的按钮（touch 事件即键事件，长按持续）。 */
    private fun button(label: String, keycode: Int, wide: Boolean = false): View {
        val view = baseButton(label, wide)
        view.setOnTouchListener { v, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    v.alpha = 0.6f
                    forwardKey(keycode, 1)
                    true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    v.alpha = 1f
                    forwardKey(keycode, 0)
                    true
                }
                else -> false
            }
        }
        return view
    }

    /** 「按住攻击」开关：开启期间持续按住左键（挖掘/攻击）。 */
    private fun holdButton(): View {
        val view = baseButton("攻击开", wide = true)
        view.setOnClickListener {
            holdLeft = !holdLeft
            (view as TextView).text = if (holdLeft) "攻击关" else "攻击开"
            GameIpc.mouse(0, if (holdLeft) 1 else 0, 0)
        }
        return view
    }

    /** 键盘开关：唤起/收起软键盘（聊天/合成台文本输入）。 */
    private fun imeButton(): View {
        val view = baseButton("键盘", wide = true)
        view.setOnClickListener {
            val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
            if (hiddenInput.visibility == View.VISIBLE) {
                hiddenInput.visibility = View.GONE
                imm?.hideSoftInputFromWindow(hiddenInput.windowToken, 0)
            } else {
                hiddenInput.visibility = View.VISIBLE
                hiddenInput.requestFocus()
                imm?.showSoftInput(hiddenInput, InputMethodManager.SHOW_IMPLICIT)
            }
        }
        return view
    }

    private fun forwardKey(keycode: Int, action: Int) {
        val glfw = GlfwKeys.toGlfwKey(keycode)
        if (glfw != 0) {
            GameIpc.key(glfw, 0, action, 0)
        }
    }
}

/** Android keycode -> GLFW 键码映射（覆盖 MC 常用键位）。 */
object GlfwKeys {
    fun toGlfwKey(keycode: Int): Int = when (keycode) {
        in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> keycode + 36 // A=29..Z=54 -> 65..90
        in KeyEvent.KEYCODE_0..KeyEvent.KEYCODE_9 -> keycode + 41 // 0=7..9=16 -> 48..57
        KeyEvent.KEYCODE_SPACE -> 32
        KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> 257
        KeyEvent.KEYCODE_ESCAPE -> 256
        KeyEvent.KEYCODE_DEL -> 259
        KeyEvent.KEYCODE_FORWARD_DEL -> 261
        KeyEvent.KEYCODE_TAB -> 258
        KeyEvent.KEYCODE_SHIFT_LEFT -> 340
        KeyEvent.KEYCODE_SHIFT_RIGHT -> 344
        KeyEvent.KEYCODE_CTRL_LEFT -> 341
        KeyEvent.KEYCODE_CTRL_RIGHT -> 345
        KeyEvent.KEYCODE_ALT_LEFT -> 342
        KeyEvent.KEYCODE_ALT_RIGHT -> 346
        KeyEvent.KEYCODE_DPAD_UP -> 265
        KeyEvent.KEYCODE_DPAD_DOWN -> 264
        KeyEvent.KEYCODE_DPAD_LEFT -> 263
        KeyEvent.KEYCODE_DPAD_RIGHT -> 262
        in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F12 -> keycode - 131 + 290
        KeyEvent.KEYCODE_MINUS -> 45
        KeyEvent.KEYCODE_EQUALS -> 61
        KeyEvent.KEYCODE_COMMA -> 44
        KeyEvent.KEYCODE_PERIOD -> 46
        KeyEvent.KEYCODE_SLASH -> 47
        KeyEvent.KEYCODE_SEMICOLON -> 59
        KeyEvent.KEYCODE_APOSTROPHE -> 39
        KeyEvent.KEYCODE_BACKSLASH -> 92
        KeyEvent.KEYCODE_GRAVE -> 96
        KeyEvent.KEYCODE_LEFT_BRACKET -> 91
        KeyEvent.KEYCODE_RIGHT_BRACKET -> 93
        else -> 0
    }

    fun mods(meta: Int): Int {
        var m = 0
        if (meta and KeyEvent.META_SHIFT_ON != 0) m = m or 0x1
        if (meta and KeyEvent.META_CTRL_ON != 0) m = m or 0x2
        if (meta and KeyEvent.META_ALT_ON != 0) m = m or 0x4
        return m
    }
}
