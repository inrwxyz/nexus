package com.inex.nexuscompanion

import android.content.Context

class Store(ctx: Context) {
    private val p = ctx.getSharedPreferences("nexus", Context.MODE_PRIVATE)

    var ip: String
        get() = p.getString("ip", "") ?: ""
        set(v) { p.edit().putString("ip", v).apply() }

    var port: Int
        get() = p.getInt("port", 41780)
        set(v) { p.edit().putInt("port", v).apply() }

    var udpPort: Int
        get() = p.getInt("udp_port", 41781)
        set(v) { p.edit().putInt("udp_port", v).apply() }

    var token: String
        get() = p.getString("token", "") ?: ""
        set(v) { p.edit().putString("token", v).apply() }

    var speed: Float
        get() = p.getFloat("speed", 1.0f)
        set(v) { p.edit().putFloat("speed", v).apply() }

    var padOn: Boolean
        get() = p.getBoolean("pad_on", false)
        set(v) { p.edit().putBoolean("pad_on", v).apply() }

    var mac: String
        get() = p.getString("mac", "") ?: ""
        set(v) { p.edit().putString("mac", v).apply() }

    var pcName: String
        get() = p.getString("pc_name", "Windows PC") ?: "Windows PC"
        set(v) { p.edit().putString("pc_name", v).apply() }

    var autoKeyboard: Boolean
        get() = p.getBoolean("auto_keyboard", false)
        set(v) { p.edit().putBoolean("auto_keyboard", v).apply() }

    var clipboardSync: Boolean
        get() = p.getBoolean("clipboard_sync", false)
        set(v) { p.edit().putBoolean("clipboard_sync", v).apply() }

    var gpProfilesJson: String
        get() = p.getString("gp_profiles_json", "") ?: ""
        set(v) { p.edit().putString("gp_profiles_json", v).apply() }

    var gpActiveProfileId: String
        get() = p.getString("gp_active_profile", "preset_action") ?: "preset_action"
        set(v) { p.edit().putString("gp_active_profile", v).apply() }

    val isPaired: Boolean get() = token.isNotEmpty() && ip.isNotEmpty()

    /** Unpair: wipe every pairing credential. UI preferences (speed, pad on/off) stay. */
    fun clearPairing() {
        p.edit().remove("ip").remove("port").remove("udp_port").remove("token").remove("mac").remove("pc_name").apply()
    }
}
