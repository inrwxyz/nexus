package com.inex.nexuscompanion

import org.json.JSONArray
import org.json.JSONObject

data class KeyBindOption(val label: String, val vk: Int)

val KEY_BIND_OPTIONS = listOf(
    KeyBindOption("W (Gas / Forward)", 'W'.code),
    KeyBindOption("A (Left)", 'A'.code),
    KeyBindOption("S (Brake / Back)", 'S'.code),
    KeyBindOption("D (Right)", 'D'.code),
    KeyBindOption("Shift (Sprint / Nitro / Sneak)", 0xA0),
    KeyBindOption("Space (Jump / Drift)", 0x20),
    KeyBindOption("Mouse Left (Fire / Mine)", 0xF1),
    KeyBindOption("Mouse Right (Aim / Place)", 0xF2),
    KeyBindOption("R (Reload)", 'R'.code),
    KeyBindOption("C (Crouch / B)", 'C'.code),
    KeyBindOption("E (Interact / Inventory)", 'E'.code),
    KeyBindOption("F (Alt / Ability)", 'F'.code),
    KeyBindOption("Q (Look / Drop)", 'Q'.code),
    KeyBindOption("V (Camera)", 'V'.code),
    KeyBindOption("G (Grenade / Extra)", 'G'.code),
    KeyBindOption("Z", 'Z'.code),
    KeyBindOption("X", 'X'.code),
    KeyBindOption("Ctrl", 0xA2),
    KeyBindOption("Tab", 0x09),
    KeyBindOption("Esc", 0x1B),
    KeyBindOption("Enter", 0x0D),
    KeyBindOption("Up Arrow", 0x26),
    KeyBindOption("Down Arrow", 0x28),
    KeyBindOption("Left Arrow", 0x25),
    KeyBindOption("Right Arrow", 0x27),
    KeyBindOption("1", '1'.code),
    KeyBindOption("2", '2'.code),
    KeyBindOption("3", '3'.code),
    KeyBindOption("4", '4'.code),
)

val HUD_SHAPE_OPTIONS = listOf("circle", "pedal", "pill", "xbox")

fun shortKeyTag(vk: Int): String = when (vk) {
    0xA0 -> "SHIFT"
    0x20 -> "SPACE"
    0xF1 -> "LMB"
    0xF2 -> "RMB"
    0xA2 -> "CTRL"
    0x09 -> "TAB"
    0x1B -> "ESC"
    0x0D -> "ENT"
    0x26 -> "UP"
    0x28 -> "DN"
    0x25 -> "LT"
    0x27 -> "RT"
    else -> if (vk in 33..126) vk.toChar().toString() else "?"
}

data class GameHudItem(
    val id: String,
    val kind: String,  // "joystick" | "button"
    val shape: String, // "circle" | "pedal" | "pill" | "xbox" | "joystick"
    val label: String,
    val nx: Float,
    val ny: Float,
    val scale: Float = 1.0f,
    val alpha: Float = 0.90f,
    val vk: Int = 0,
    val accentArgb: Long = 0xFF262A34,
)

data class GameProfile(
    val id: String,
    val name: String,
    val category: String = "custom",
    val description: String = "",
    val gyroEnabled: Boolean = false,
    val items: List<GameHudItem>,
)

data class RemoteGamepadStore(
    val activeProfileId: String,
    val detectedGame: String?,
    val profiles: List<GameProfile>,
)

fun builtInPresets(): List<GameProfile> = listOf(
    GameProfile(
        id = "preset_action",
        name = "Action / Shooter",
        category = "action",
        description = "Mobile shooter HUD",
        gyroEnabled = false,
        items = listOf(
            GameHudItem("joy_wasd", "joystick", "joystick", "MOVE", 0.16f, 0.67f, 1.25f, 0.90f, 0, 0xFF1F242D),
            GameHudItem("btn_run", "button", "pill", "RUN", 0.14f, 0.30f, 0.95f, 0.86f, 0xA0, 0xFF262A34),
            GameHudItem("btn_use", "button", "circle", "USE", 0.63f, 0.30f, 0.88f, 0.86f, 'E'.code, 0xFF262A34),
            GameHudItem("btn_aim", "button", "circle", "AIM", 0.70f, 0.51f, 1.00f, 0.88f, 0xF2, 0xFF2D3340),
            GameHudItem("btn_fire", "button", "circle", "FIRE", 0.84f, 0.66f, 1.46f, 0.94f, 0xF1, 0xFFB8332A),
            GameHudItem("btn_jump", "button", "circle", "JUMP", 0.90f, 0.37f, 1.02f, 0.88f, 0x20, 0xFF2E5C48),
            GameHudItem("btn_reload", "button", "circle", "RELOAD", 0.74f, 0.84f, 0.90f, 0.86f, 'R'.code, 0xFF262A34),
            GameHudItem("btn_crouch", "button", "circle", "CROUCH", 0.91f, 0.84f, 0.90f, 0.86f, 'C'.code, 0xFF262A34),
            GameHudItem("btn_esc", "button", "pill", "ESC", 0.93f, 0.14f, 0.74f, 0.80f, 0x1B, 0xFF262A34),
        ),
    ),
    GameProfile(
        id = "preset_racing",
        name = "Racing Wheel Pro",
        category = "racing",
        description = "Sculpted Gas & Brake pedals + Gyro Wheel",
        gyroEnabled = true,
        items = listOf(
            GameHudItem("btn_left", "button", "pill", "◀ STEER", 0.12f, 0.68f, 1.40f, 0.90f, 'A'.code, 0xFF222834),
            GameHudItem("btn_right", "button", "pill", "STEER ▶", 0.27f, 0.68f, 1.40f, 0.90f, 'D'.code, 0xFF222834),
            GameHudItem("btn_look", "button", "pill", "LOOK", 0.12f, 0.28f, 0.88f, 0.82f, 'Q'.code, 0xFF262A34),
            GameHudItem("btn_cam", "button", "pill", "CAM", 0.26f, 0.28f, 0.88f, 0.82f, 'V'.code, 0xFF262A34),
            GameHudItem("btn_brake", "button", "pedal", "BRAKE", 0.71f, 0.71f, 1.38f, 0.94f, 'S'.code, 0xFFB8332A),
            GameHudItem("btn_gas", "button", "pedal", "GAS", 0.88f, 0.67f, 1.56f, 0.96f, 'W'.code, 0xFF1F7A52),
            GameHudItem("btn_drift", "button", "pill", "DRIFT", 0.72f, 0.34f, 1.04f, 0.90f, 0x20, 0xFF334155),
            GameHudItem("btn_nitro", "button", "pill", "NITRO", 0.88f, 0.31f, 1.12f, 0.92f, 0xA0, 0xFF1D4ED8),
            GameHudItem("btn_esc", "button", "pill", "ESC", 0.93f, 0.13f, 0.74f, 0.80f, 0x1B, 0xFF262A34),
        ),
    ),
    GameProfile(
        id = "preset_xbox",
        name = "Xbox Pro Gamepad",
        category = "console",
        description = "ABXY color cluster & triggers",
        gyroEnabled = false,
        items = listOf(
            GameHudItem("joy_ls", "joystick", "joystick", "L STICK", 0.16f, 0.67f, 1.28f, 0.92f, 0, 0xFF1F242D),
            GameHudItem("btn_lt", "button", "pill", "LT", 0.11f, 0.25f, 0.96f, 0.88f, 0xF2, 0xFF262A34),
            GameHudItem("btn_lb", "button", "pill", "LB", 0.24f, 0.25f, 0.96f, 0.88f, 'Q'.code, 0xFF262A34),
            GameHudItem("btn_rb", "button", "pill", "RB", 0.76f, 0.25f, 0.96f, 0.88f, 'F'.code, 0xFF262A34),
            GameHudItem("btn_rt", "button", "pill", "RT", 0.89f, 0.25f, 0.96f, 0.88f, 0xF1, 0xFF1D4ED8),
            GameHudItem("btn_y", "button", "xbox", "Y", 0.82f, 0.45f, 1.00f, 0.94f, 'E'.code, 0xFFD97706),
            GameHudItem("btn_x", "button", "xbox", "X", 0.73f, 0.62f, 1.00f, 0.94f, 'R'.code, 0xFF2563EB),
            GameHudItem("btn_b", "button", "xbox", "B", 0.91f, 0.62f, 1.00f, 0.94f, 'C'.code, 0xFFDC2626),
            GameHudItem("btn_a", "button", "xbox", "A", 0.82f, 0.79f, 1.04f, 0.96f, 0x20, 0xFF16A34A),
            GameHudItem("btn_back", "button", "pill", "TAB", 0.42f, 0.88f, 0.76f, 0.80f, 0x09, 0xFF262A34),
            GameHudItem("btn_start", "button", "pill", "MENU", 0.58f, 0.88f, 0.76f, 0.80f, 0x1B, 0xFF262A34),
        ),
    ),
)

object GameProfileStore {
    fun load(rawJson: String): List<GameProfile> {
        val defaults = builtInPresets()
        if (rawJson.isBlank()) return defaults
        return try {
            val arr = JSONArray(rawJson)
            val loaded = mutableListOf<GameProfile>()
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                val itemsArr = o.optJSONArray("items") ?: JSONArray()
                val items = mutableListOf<GameHudItem>()
                for (j in 0 until itemsArr.length()) {
                    val it = itemsArr.getJSONObject(j)
                    val kind = it.optString("kind", "button")
                    items += GameHudItem(
                        id = it.optString("id", "btn_$j"),
                        kind = kind,
                        shape = it.optString("shape", if (kind == "joystick") "joystick" else "circle"),
                        label = it.optString("label", "BTN"),
                        nx = it.optDouble("nx", 0.5).toFloat().coerceIn(0.06f, 0.94f),
                        ny = it.optDouble("ny", 0.5).toFloat().coerceIn(0.10f, 0.92f),
                        scale = it.optDouble("scale", 1.0).toFloat().coerceIn(0.6f, 1.9f),
                        alpha = it.optDouble("alpha", 0.90).toFloat().coerceIn(0.3f, 1.0f),
                        vk = it.optInt("vk", 0),
                        accentArgb = it.optLong("accent", 0xFF262A34),
                    )
                }
                val id = o.optString("id")
                val name = o.optString("name")
                if (id.isNotBlank() && name.isNotBlank() && items.isNotEmpty()) {
                    loaded += GameProfile(
                        id = id,
                        name = name,
                        category = o.optString("category", "custom"),
                        description = o.optString("description", ""),
                        gyroEnabled = o.optBoolean("gyro", false),
                        items = items,
                    )
                }
            }
            if (loaded.isEmpty()) defaults else {
                val byId = loaded.associateBy { it.id }
                defaults.map { byId[it.id] ?: it } + loaded.filter { p -> defaults.none { it.id == p.id } && !p.id.startsWith("preset_") }
            }
        } catch (_: Exception) {
            defaults
        }
    }

    fun profileToJson(p: GameProfile): JSONObject {
        val itemsArr = JSONArray()
        for (it in p.items) {
            itemsArr.put(
                JSONObject()
                    .put("id", it.id)
                    .put("kind", it.kind)
                    .put("shape", it.shape)
                    .put("label", it.label)
                    .put("nx", it.nx.toDouble())
                    .put("ny", it.ny.toDouble())
                    .put("scale", it.scale.toDouble())
                    .put("alpha", it.alpha.toDouble())
                    .put("vk", it.vk)
                    .put("accent", it.accentArgb),
            )
        }
        return JSONObject()
            .put("id", p.id)
            .put("name", p.name)
            .put("category", p.category)
            .put("description", p.description)
            .put("gyro", p.gyroEnabled)
            .put("items", itemsArr)
    }

    fun serialize(profiles: List<GameProfile>): String {
        val arr = JSONArray()
        for (p in profiles) arr.put(profileToJson(p))
        return arr.toString()
    }
}
