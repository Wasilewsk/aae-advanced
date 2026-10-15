package io.github.aaron_gh.aae.remote

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
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
}
