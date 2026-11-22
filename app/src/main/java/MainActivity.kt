package com.opentube.app

import android.app.Activity
import android.os.Bundle
import android.widget.TextView

class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Step 1 smoke test: hello() comes from the UniFFI bindings (SnapFfi).
        setContentView(TextView(this).apply { text = SnapFfi.hello() })
    }
}
