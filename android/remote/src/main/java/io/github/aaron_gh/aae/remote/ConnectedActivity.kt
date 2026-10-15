package io.github.aaron_gh.aae.remote

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import org.json.JSONArray
import org.json.JSONObject

/**
 * A screen that needs the connection to the computer. If it ends, the screen
 * says why and goes back to the computers.
 */
abstract class ConnectedActivity : Activity() {
    protected lateinit var ui: Ui

    private val watcher = object : Connection.Listener {
        override fun closed(reason: String) = lost(reason)
        override fun event(name: String, data: JSONObject) = onEvent(name, data)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Ui(this)
        if (Remote.connection == null) {
            finish()
            return
        }
        Remote.watch(watcher)
    }

    override fun onDestroy() {
        Remote.unwatch(watcher)
        super.onDestroy()
    }

    open fun onEvent(name: String, data: JSONObject) {}

    private fun lost(reason: String) {
        if (isFinishing) return
        ui.message("Disconnected", "The connection to the computer ended. $reason") {
            startActivity(
                Intent(this, ComputersActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            )
            finish()
        }
    }

    /** Calls the computer, saying progress and any failure in the status line. */
    protected fun call(
        method: String,
        params: JSONObject = JSONObject(),
        failed: () -> Unit = {},
        done: (Any?) -> Unit = {},
    ) {
        val connection = Remote.connection ?: run {
            ui.say("Not connected to the computer.")
            failed()
            return
        }
        connection.call(method, params, progress = { message, percent ->
            when {
                message != null -> ui.say(message)
                percent != null && percent % 10 == 0 -> ui.say("$percent percent.")
            }
        }) { result ->
            result.onSuccess(done).onFailure {
                ui.say(it.message ?: "That didn't work.")
                failed()
            }
        }
    }

    /** A file's name, as the app it came from gives it. */
    protected fun fileName(uri: Uri, otherwise: String = "app.apk"): String =
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
        } ?: otherwise

    /** Sends a file to the computer; [sent] gets its upload number. */
    protected fun send(uri: Uri, name: String, sent: (Int) -> Unit) {
        val connection = Remote.connection ?: return
        ui.say("Sending $name.")
        connection.call("upload.begin", JSONObject().put("name", name)) { begun ->
            val number = begun.getOrNull()?.toString()?.toIntOrNull()
                ?: return@call ui.say("The computer couldn't take the file: ${begun.exceptionOrNull()?.message}")
            Thread {
                val ok = try {
                    contentResolver.openInputStream(uri)?.use { input ->
                        val buffer = ByteArray(256 * 1024)
                        while (true) {
                            val read = input.read(buffer)
                            if (read < 0) break
                            if (!connection.uploadPart(number, buffer, read)) return@use false
                        }
                        true
                    } ?: false
                } catch (_: Exception) {
                    false
                }
                runOnUiThread {
                    if (!ok) return@runOnUiThread ui.say("$name couldn't be sent.")
                    sent(number)
                }
            }.start()
        }
    }

    /** Asks for an APK; [onActivityResult] gets it with [request]. */
    protected fun chooseApk(request: Int) {
        val pick = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
            .setType("application/vnd.android.package-archive")
        startActivityForResult(pick, request)
    }

    /** Sends an APK to the computer, which installs it on [device], then asks about its parts. */
    protected fun installApp(device: String, uri: Uri) {
        val name = fileName(uri)
        send(uri, name) { number ->
            ui.say("Installing $name.")
            call("tools.install", JSONObject().put("id", device).put("upload", number)) { result ->
                val json = result as JSONObject
                ui.say("Installed ${json.optString("package")}.")
                askParts(device, json.optJSONArray("parts") ?: JSONArray())
            }
        }
    }

    /** Asks which of the app's special parts to turn on, such as an accessibility service. */
    private fun askParts(device: String, parts: JSONArray) {
        val list = (0 until parts.length()).map { parts.getJSONObject(it) }.filter { it.isNull("choice") }
        if (list.isEmpty()) return
        val on = BooleanArray(list.size) { false }
        AlertDialog.Builder(this)
            .setTitle("Turn These On?")
            .setMultiChoiceItems(list.map { "${it.optString("name")}, ${it.optString("kind")}" }.toTypedArray(), on) { _, which, checked ->
                on[which] = checked
            }
            .setPositiveButton("Done") { _, _ ->
                val choices = JSONArray()
                list.forEachIndexed { i, p -> choices.put(JSONObject(p.toString()).put("on", on[i])) }
                call("tools.app.choices", JSONObject().put("id", device).put("choices", choices)) { ui.say("Done.") }
            }
            .show()
    }

    /**
     * Sends an APK to the computer, which makes it [device]'s screen reader,
     * or installs it when the device next starts. A build signed differently
     * from the one installed replaces it, losing its settings, if confirmed.
     */
    protected fun installScreenReader(device: String, deviceName: String, uri: Uri, replace: Boolean = false) {
        val connection = Remote.connection ?: return ui.say("Not connected to the computer.")
        val name = fileName(uri, "screen-reader.apk")
        send(uri, name) { number ->
            ui.say("Installing $name.")
            val params = JSONObject().put("id", device).put("upload", number).put("replace", replace)
            connection.call("device.screen_reader.install", params) { result ->
                result.onSuccess { ui.say(it.toString()) }.onFailure {
                    val message = it.message ?: "That didn't work."
                    if (replace || !message.contains("signed differently")) return@onFailure ui.say(message)
                    ui.confirm(
                        "Replace the Screen Reader on $deviceName?",
                        "This build is signed differently from the one on $deviceName, so it can't be installed over it. Replacing it removes the old one first, and with it the screen reader's settings.",
                        "Replace",
                    ) { installScreenReader(device, deviceName, uri, replace = true) }
                }
            }
        }
    }
}
