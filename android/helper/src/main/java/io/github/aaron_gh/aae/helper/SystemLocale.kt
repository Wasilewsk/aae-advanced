package io.github.aaron_gh.aae.helper

import android.content.res.Configuration
import android.os.Build
import android.os.LocaleList
import java.util.Locale

/**
 * The device's languages, as Settings sets them: "locale" prints them as
 * language tags, such as "fr-FR,en-US", and "locale fr-FR,en-US" sets them.
 * Android 6 and earlier have one language, so only the first is set there.
 *
 * Settings changes them through the activity manager's hidden interface,
 * which needs the CHANGE_CONFIGURATION permission. The shell user has it.
 */
object SystemLocale {
    fun run(tags: String?) {
        val am = activityManager()
        if (tags == null) {
            val config = am.javaClass.getMethod("getConfiguration").invoke(am) as Configuration
            println(if (Build.VERSION.SDK_INT >= 24) config.locales.toLanguageTags() else config.locale.toLanguageTag())
            return
        }
        val config = Configuration()
        val set = if (Build.VERSION.SDK_INT >= 24) {
            val locales = LocaleList.forLanguageTags(tags)
            if (locales.isEmpty) throw IllegalArgumentException("no languages in \"$tags\"")
            config.setLocales(locales)
            locales.toLanguageTags()
        } else {
            val first = tags.split(',').map(String::trim).firstOrNull { it.isNotEmpty() }
                ?: throw IllegalArgumentException("no languages in \"$tags\"")
            config.locale = Locale.forLanguageTag(first)
            config.locale.toLanguageTag()
        }
        // Marks it as the user's choice, as Settings does, so it's kept.
        Configuration::class.java.getField("userSetLocale").setBoolean(config, true)
        val withAttribution = am.javaClass.methods.firstOrNull {
            it.name == "updatePersistentConfigurationWithAttribution" && it.parameterTypes.size == 3
        }
        if (withAttribution != null) {
            withAttribution.invoke(am, config, "com.android.shell", null)
        } else {
            am.javaClass.getMethod("updatePersistentConfiguration", Configuration::class.java).invoke(am, config)
        }
        println(set)
    }

    /** The activity manager's hidden interface. Android 7 and earlier get it from ActivityManagerNative. */
    private fun activityManager(): Any =
        if (Build.VERSION.SDK_INT >= 26) {
            Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null)!!
        } else {
            Class.forName("android.app.ActivityManagerNative").getMethod("getDefault").invoke(null)!!
        }
}
