package com.quartz.launcher

import android.app.AlertDialog
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
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
import android.widget.SeekBar
import android.widget.TextView

/**
 * 游戏内虚拟控制层（v0.1.5，v0.2.1-f 对标 FCL 完整化）：
 * - 左下方向键（W/A/S/D）+ 跳跃 + 潜行 + 疾跑（双击 W 或独立开关）；
 * - 右下动作键：跳跃、丢弃（Q）、副手（F）、「按住攻击」开关；
 * - 右边缘：热键栏 1-9 快捷键列；
 * - 左上系统键：疾跑开关、F3 调试、F5 视角、中键（选取方块）；
 * - 顶部右侧：Esc（暂停）、E（背包）、T（聊天）、键盘、灵敏度；
 * - 滚轮按钮（▲▼）：热键栏切换（grab 模式下双指被右键占用）；
 * - 虚拟光标可视化（未 grab 菜单模式）：跟随触摸移动的鼠标箭头。
 * 全部按键经 GameIpc（Messenger IPC）-> :game 进程桥 -> LWJGL 键回调进入游戏。
 */
class GameControls(
    context: Context,
    private val gameView: GameView?
) : FrameLayout(context) {

    private val hiddenInput: EditText
    private var holdLeft = false
    private var sprinting = false
    private var lastWDownAt = 0L
    private var wDown = false

    /** 虚拟光标视图（未 grab 菜单模式显示）。 */
    private val cursorView: View
    /** 滚轮/热键列（grab 模式才显示，菜单模式隐藏避免误触）。 */
    private val wheelCol: LinearLayout
    private val hotbarCol: LinearLayout

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

        // ---- 虚拟光标（菜单模式常驻，跟随 GameView 触摸）----
        cursorView = CursorArrow(context).apply {
            layoutParams = LayoutParams(dp(28), dp(28))
            translationX = (resources.displayMetrics.widthPixels - dp(28)) / 2f
            translationY = (resources.displayMetrics.heightPixels - dp(28)) / 2f
            elevation = dp(8).toFloat()
        }
        addView(cursorView)

        // ---- 顶部右侧功能键 ----
        val topRow = row().apply {
            gravity = Gravity.TOP or Gravity.END
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
            setPadding(0, dp(8), dp(8), 0)
        }
        topRow.addView(button("Esc", KeyEvent.KEYCODE_ESCAPE, small = true))
        topRow.addView(button("背包", KeyEvent.KEYCODE_E, small = true))
        topRow.addView(button("聊天", KeyEvent.KEYCODE_T, small = true))
        topRow.addView(imeButton(small = true))
        topRow.addView(sensButton())
        addView(topRow)

        // ---- 左上系统键（疾跑/调试/视角/中键）----
        val sysCol = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.TOP or Gravity.START
            )
            setPadding(dp(8), dp(8), 0, 0)
        }
        sysCol.addView(sprintToggle())
        sysCol.addView(button("调试", KeyEvent.KEYCODE_F3, small = true))
        sysCol.addView(button("视角", KeyEvent.KEYCODE_F5, small = true))
        sysCol.addView(pickButton())
        addView(sysCol)

        // ---- 左下方向键（W 双击 = 疾跑）----
        val dpad = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.START
            )
            setPadding(dp(10), 0, 0, dp(12))
        }
        dpad.addView(row().apply { addView(wButton()) })
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
            setPadding(0, 0, dp(10), dp(12))
        }
        actions.addView(button("跳跃", KeyEvent.KEYCODE_SPACE, wide = true))
        actions.addView(button("潜行", KeyEvent.KEYCODE_SHIFT_LEFT, wide = true))
        actions.addView(row().apply {
            addView(button("丢弃", KeyEvent.KEYCODE_Q, small = true))
            addView(button("副手", KeyEvent.KEYCODE_F, small = true))
        })
        actions.addView(holdButton())
        addView(actions)

        // ---- 右边缘热键栏 1-9 ----
        hotbarCol = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.CENTER or Gravity.END
            )
            setPadding(0, 0, dp(4), 0)
        }
        for (n in 1..9) {
            hotbarCol.addView(digitButton(n))
        }
        addView(hotbarCol)

        // ---- 滚轮列（grab 模式专用：热键栏切换）----
        wheelCol = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.CENTER or Gravity.START
            )
            setPadding(dp(4), 0, 0, 0)
        }
        wheelCol.addView(wheelButton("▲", +1.0))
        wheelCol.addView(wheelButton("▼", -1.0))
        addView(wheelCol)
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        // 光标初始居中（横屏旋转后也随新尺寸重算）
        if (cursorView.visibility == View.VISIBLE) {
            cursorView.translationX = (w - cursorView.layoutParams.width) / 2f
            cursorView.translationY = (h - cursorView.layoutParams.height) / 2f
        }
    }

    /** grab 状态切换：菜单模式显示光标，游戏内显示滚轮列+热键栏。 */
    fun setGrabbed(grabbed: Boolean) {
        cursorView.visibility = if (grabbed) View.GONE else View.VISIBLE
        wheelCol.visibility = if (grabbed) View.VISIBLE else View.GONE
        hotbarCol.visibility = if (grabbed) View.VISIBLE else View.GONE
        alpha = if (grabbed) 1f else 0.85f
    }

    /** GameView 未 grab 触摸 -> 光标位置（触摸即指针）。 */
    fun moveCursor(x: Float, y: Float) {
        if (GameHost.grabbed) return
        cursorView.translationX = x - dp(28) / 2f
        cursorView.translationY = y - dp(2)
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

    private fun baseButton(label: String, wide: Boolean = false, small: Boolean = false): TextView {
        return TextView(context).apply {
            text = label
            gravity = Gravity.CENTER
            textSize = if (small) 11f else 13f
            setTextColor(0xFFE8EBF1.toInt())
            typeface = Typeface.DEFAULT_BOLD
            background = keyStyle()
            val w = when {
                wide -> dp(96)
                small -> dp(46)
                else -> dp(56)
            }
            val h = if (small) dp(40) else dp(50)
            layoutParams = LinearLayout.LayoutParams(w, h).apply {
                setMargins(dp(4), dp(4), dp(4), dp(4))
            }
        }
    }

    /** 按下/抬起键的按钮（touch 事件即键事件，长按持续）。 */
    private fun button(
        label: String,
        keycode: Int,
        wide: Boolean = false,
        small: Boolean = false,
        hide: Boolean = false
    ): View {
        val view = baseButton(label, wide, small)
        if (hide) view.visibility = View.GONE
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

    /** W 键（带双击疾跑：300ms 内二次按下 -> Ctrl 按住）。 */
    private fun wButton(): View {
        val view = baseButton("W")
        view.setOnTouchListener { v, ev ->
            when (ev.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    v.alpha = 0.6f
                    val now = System.currentTimeMillis()
                    if (!sprinting && now - lastWDownAt < 300) {
                        sprinting = true
                        forwardKey(KeyEvent.KEYCODE_CTRL_LEFT, 1)
                    }
                    lastWDownAt = now
                    wDown = true
                    forwardKey(KeyEvent.KEYCODE_W, 1)
                    true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    v.alpha = 1f
                    wDown = false
                    forwardKey(KeyEvent.KEYCODE_W, 0)
                    if (sprinting) {
                        sprinting = false
                        forwardKey(KeyEvent.KEYCODE_CTRL_LEFT, 0)
                    }
                    true
                }
                else -> false
            }
        }
        return view
    }

    /** 疾跑开关（按住 Ctrl，配合方向键前进触发疾跑）。 */
    private fun sprintToggle(): View {
        val view = baseButton("疾跑", small = true)
        view.setOnClickListener {
            sprinting = !sprinting
            (view as TextView).text = if (sprinting) "疾跑中" else "疾跑"
            view.alpha = if (sprinting) 0.6f else 1f
            forwardKey(KeyEvent.KEYCODE_CTRL_LEFT, if (sprinting) 1 else 0)
        }
        return view
    }

    /** 中键（选取方块）。 */
    private fun pickButton(): View {
        val view = baseButton("取方", small = true)
        view.setOnClickListener { GameIpc.mouse(2, 1, 0); GameIpc.mouse(2, 0, 0) }
        return view
    }

    /** 热键栏数字键（按下发数字键 click）。 */
    private fun digitButton(n: Int): View {
        val view = baseButton(n.toString(), small = true)
        view.layoutParams = (view.layoutParams as LinearLayout.LayoutParams).apply {
            setMargins(dp(3), dp(3), dp(3), dp(3))
        }
        view.setOnClickListener {
            val glfw = GlfwKeys.toGlfwKey(KeyEvent.KEYCODE_0 + n)
            if (glfw != 0) {
                GameIpc.key(glfw, 0, 1, 0)
                GameIpc.key(glfw, 0, 0, 0)
            }
        }
        return view
    }

    /** 滚轮按钮（▲=上滚热键栏 +1，▼=-1；MC 滚轮方向：dy<0 为下一切换）。 */
    private fun wheelButton(label: String, dir: Double): View {
        val view = baseButton(label, small = true)
        view.setOnClickListener { GameIpc.scroll(0.0, dir) }
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
    private fun imeButton(small: Boolean = false): View {
        val view = baseButton("键盘", small = small)
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

    /** 灵敏度面板：SeekBar 调视角灵敏度（0.2-3.0）。 */
    private fun sensButton(): View {
        val view = baseButton("灵敏度", small = true)
        view.setOnClickListener {
            val gv = gameView ?: return@setOnClickListener
            val seek = SeekBar(context).apply {
                max = 280
                progress = ((gv.lookSensitivity - 0.2f) / 2.8f * 280).toInt()
                setPadding(dp(24), dp(16), dp(24), 0)
            }
            val label = TextView(context).apply {
                text = "视角灵敏度：%.2f".format(gv.lookSensitivity)
                setPadding(dp(24), dp(12), dp(24), 0)
            }
            seek.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(sb: SeekBar?, p: Int, fromUser: Boolean) {
                    gv.lookSensitivity = 0.2f + p / 280f * 2.8f
                    label.text = "视角灵敏度：%.2f".format(gv.lookSensitivity)
                }
                override fun onStartTrackingTouch(sb: SeekBar?) {}
                override fun onStopTrackingTouch(sb: SeekBar?) {}
            })
            AlertDialog.Builder(context)
                .setTitle("触控设置")
                .setView(LinearLayout(context).apply {
                    orientation = LinearLayout.VERTICAL
                    addView(label)
                    addView(seek)
                })
                .setPositiveButton("完成", null)
                .show()
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

/** 虚拟鼠标箭头（白描边黑填充，经典指针形状）。 */
class CursorArrow(context: Context) : View(context) {
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.WHITE }
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.BLACK
        style = Paint.Style.STROKE
        strokeWidth = 2f
    }
    private val path = Path()

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val w = width.toFloat()
        val h = height.toFloat()
        path.reset()
        path.moveTo(w * 0.18f, h * 0.06f)
        path.lineTo(w * 0.18f, h * 0.72f)
        path.lineTo(w * 0.40f, h * 0.56f)
        path.lineTo(w * 0.54f, h * 0.90f)
        path.lineTo(w * 0.66f, h * 0.84f)
        path.lineTo(w * 0.52f, h * 0.52f)
        path.lineTo(w * 0.78f, h * 0.50f)
        path.close()
        canvas.drawPath(path, fill)
        canvas.drawPath(path, stroke)
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
