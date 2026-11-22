package com.opentube.app

import android.os.Bundle
import android.util.Log
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import com.opentube.ffi.SnapFfi

class MainActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        WindowCompat.setDecorFitsSystemWindows(window, false)

        val pad = (16 * resources.displayMetrics.density).toInt()
        val text = TextView(this).apply {
            textSize = 16f
            setPadding(pad, pad, pad, pad)
        }
        setContentView(text)

        ViewCompat.setOnApplyWindowInsetsListener(text) { view, insets ->
            val bars = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
            )
            view.setPadding(pad + bars.left, pad + bars.top, pad + bars.right, pad + bars.bottom)
            insets
        }

        // A missing or ABI-incompatible .so must not kill the process on boot.
        text.text = runCatching { SnapFfi.hello() }.getOrElse { e ->
            Log.e("OpenTube", "native library unavailable", e)
            getString(R.string.native_init_failed)
        }
    }
}
