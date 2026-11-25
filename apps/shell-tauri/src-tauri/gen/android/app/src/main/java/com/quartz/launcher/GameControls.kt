package com.quartz.launcher

import android.app.AlertDialog
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView

/**
 * 游戏内虚拟控制层（v0.1.5，v0.2.3 布局重排）：<br>
 * - 菜单模式（未 grab）：仅左上系统键 + 顶部功能键 + 虚拟光标；移动/动作/热键全部隐藏（对标 FCL）；<br>
 * - 游戏内（grab）：左下方向键（W/A/S/D）+ 疾跑（双击 W）；右下动作键：跳跃、潜行、丢弃（Q）、副手（F）、「按住攻击」；<br>
 * - 底部中央：热键 1-9 + 滚轮 ▲▼ 横排（避开 MC 自绘热栏与系统手势条）；<br>
 * - 左上系统键：疾跑开关、F3 调试、F5 视角、中键（选取方块）；<br>
 * - 顶部右侧：Esc（暂停）、E（背包）、T（聊天）、键盘、灵敏度；<br>
 * - 虚拟光标可视化（未 grab 菜单模式）：跟随触摸移动的鼠标箭头；<br>
 * - 系统栏避让：容器按 WindowInsets 打 padding（状态栏/手势条/刘海不遮挡按键）。<br>
 * v0.2.3 修复：热键列与动作键在右缘重叠、滚轮列与左上系统键重叠、初始未隐藏
 * grab 专用控件 —— 实机截图确认的三处「按键错位」。全部按键经 GameIpc
 * （Messenger IPC）-> :game 进程桥 -> LWJGL 键回调进入游戏。
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
    /** 光标是否已跟随过触摸（跟随过就不再被 insets 变化重新居中）。 */
    private var cursorFollowed = false
    /** 底部中央热键横排（1-9 + 滚轮 ▲▼，仅 grab 模式显示）。 */
    private val hotbarRow: LinearLayout
    /** 移动键（W/A/S/D，仅 grab 模式显示）。 */
    private val dpad: LinearLayout
    /** 右下动作键（跳跃/潜行/丢弃/副手/攻击，仅 grab 模式显示）。 */
    private val actions: LinearLayout

    init {
        // v0.2.9：光标允许画进 padding 区（状态栏/手势条上层），边缘触摸不裁剪
        clipChildren = false
        clipToPadding = false

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

        // ---- 系统栏避让：容器 padding = 系统窗口 insets（状态栏/手势条/刘海）----
        setOnApplyWindowInsetsListener { v, insets ->
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                val bars = insets.getInsets(
                    WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout()
                )
                v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            } else {
                @Suppress("DEPRECATION")
                v.setPadding(
                    insets.systemWindowInsetLeft,
                    insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight,
                    insets.systemWindowInsetBottom
                )
            }
            // v0.2.9：padding 变化会平移光标（子视图位置 = padding + translation），
            // 用户尚未触摸时重新居中一次；已跟随触摸则不扰动
            if (!cursorFollowed && cursorView.visibility == View.VISIBLE) centerCursor()
            insets
        }

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

        // ---- 左下方向键（W 双击 = 疾跑；仅 grab 模式显示）----
        dpad = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.START
            )
            setPadding(dp(10), 0, 0, dp(12))
        }
        // v0.2.9：W 行宽度 MATCH_PARENT + 行内居中 —— 修复 W 左对齐在 A 正上方
        //（十字散架，实机截图确认），现在 W 居中在 S 正上方（FCL 同款十字）
        dpad.addView(
            row().apply {
                gravity = Gravity.CENTER_HORIZONTAL
                addView(wButton())
            },
            LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT
            )
        )
        dpad.addView(row().apply {
            addView(button("A", KeyEvent.KEYCODE_A))
            addView(button("S", KeyEvent.KEYCODE_S))
            addView(button("D", KeyEvent.KEYCODE_D))
        })
        addView(dpad)

        // ---- 右下动作键（仅 grab 模式显示）----
        actions = LinearLayout(context).apply {
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

        // ---- 底部中央热键横排（1-9 + 滚轮 ▲▼，仅 grab 模式显示）----
        // 旧版为右缘竖排热键列 + 左缘竖排滚轮列，与右下动作键/左上系统键重叠
        //（实机截图确认），且底部贴边被手势条遮挡 —— 合并为底中央横排，
        // 抬高避开 MC 自绘热栏。
        hotbarRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            layoutParams = FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL
            ).apply {
                bottomMargin = dp(64) // 抬过 MC 自绘热栏（22 gui px × 缩放）
            }
        }
        hotbarRow.addView(wheelButton("▲", +1.0))
        for (n in 1..9) {
            hotbarRow.addView(digitButton(n))
        }
        hotbarRow.addView(wheelButton("▼", -1.0))
        addView(hotbarRow)

        // ---- 初始为菜单模式：光标可见，grab 专用控件全部隐藏 ----
        // 此前仅靠游戏 grab 回调切换，主菜单阶段从未回调 → 热键/滚轮列
        // 以默认 VISIBLE 状态叠在菜单上（实机截图确认）。
        setGrabbed(false)
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        if (cursorView.visibility != View.VISIBLE) return
        if (!cursorFollowed) {
            // 光标初始居中（横屏旋转后也随新尺寸重算）
            centerCursor()
        } else {
            // v0.2.10：已跟随过的光标按新旧尺寸比例重映射并夹取回屏幕，
            // 防止 ROM 瞬时旋转/insets 变化后指针留在旧坐标 —— 绝对位置超出
            // 新尺寸 = 光标画到屏外（用户体感「虚拟鼠标在下方屏幕不显示」）
            remapCursor(oldw, oldh)
        }
    }

    /** v0.2.10：尺寸变化时把已跟随光标按中心点比例重映射到新 bounds。 */
    private fun remapCursor(oldw: Int, oldh: Int) {
        val cw = cursorView.layoutParams.width
        val ch = cursorView.layoutParams.height
        if (oldw <= 0 || oldh <= 0 || width <= 0 || height <= 0) {
            clampCursor()
            return
        }
        val absX = paddingLeft + cursorView.translationX + cw / 2f
        val absY = paddingTop + cursorView.translationY + ch / 2f
        val nx = absX * width / oldw
        val ny = absY * height / oldh
        cursorView.translationX = nx - cw / 2f - paddingLeft
        cursorView.translationY = ny - ch / 2f - paddingTop
        clampCursor()
    }

    /**
     * v0.2.10：把光标夹取回容器可见范围（绝对坐标 [0, width-cw]）。
     * v0.2.9 起光标允许画进 insets padding 区（clipChildren=false），因此
     * 这里夹取的是整屏 bounds 而非 padding 内容区 —— 只保证不出屏。
     */
    private fun clampCursor() {
        val cw = cursorView.layoutParams.width
        val ch = cursorView.layoutParams.height
        val minTX = -paddingLeft.toFloat()
        val maxTX = (width - cw - paddingLeft).coerceAtLeast(minTX).toFloat()
        val minTY = -paddingTop.toFloat()
        val maxTY = (height - ch - paddingTop).coerceAtLeast(minTY).toFloat()
        cursorView.translationX = cursorView.translationX.coerceIn(minTX, maxTX)
        cursorView.translationY = cursorView.translationY.coerceIn(minTY, maxTY)
    }

    /** 光标居中于容器 bounds：子视图位置 = padding + translation，须扣 padding。 */
    private fun centerCursor() {
        cursorView.translationX = (width - cursorView.layoutParams.width) / 2f - paddingLeft
        cursorView.translationY = (height - cursorView.layoutParams.height) / 2f - paddingTop
    }

    /** grab 状态切换：菜单模式显示光标（隐藏移动/动作/热键），游戏内反之。 */
    fun setGrabbed(grabbed: Boolean) {
        cursorView.visibility = if (grabbed) View.GONE else View.VISIBLE
        hotbarRow.visibility = if (grabbed) View.VISIBLE else View.GONE
        dpad.visibility = if (grabbed) View.VISIBLE else View.GONE
        actions.visibility = if (grabbed) View.VISIBLE else View.GONE
        alpha = if (grabbed) 1f else 0.85f
    }

    /**
     * GameView 未 grab 触摸 -> 光标位置（触摸即指针）。
     *
     * v0.2.9 修复（实机：下半屏/右缘光标消失）：x/y 是 GameView 全屏坐标，
     * 而 cursorView 挂在本容器的 insets padding 内容区 —— FrameLayout 子视图
     * 位置 = padding + translation，不扣 padding 则指针被状态栏/挖孔/手势条
     * insets 整体偏移，触摸越靠下/靠右越偏，到底部直接越界出屏（叠加默认
     * clipChildren 裁剪 = 指针消失）。扣除 padding 后坐标 1:1 对齐触摸点。
     */
    fun moveCursor(x: Float, y: Float) {
        if (GameHost.grabbed) return
        cursorFollowed = true
        cursorView.translationX = x - dp(28) / 2f - paddingLeft
        cursorView.translationY = y - dp(2) - paddingTop
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
