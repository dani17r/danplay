package local.danplay.pistas

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import android.provider.MediaStore
import android.webkit.JavascriptInterface
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import org.json.JSONArray
import org.json.JSONObject

/**
 * La música del teléfono (src/movil, «Música del teléfono»): el permiso de
 * leer «música y audio» para abrir las canciones con su ruta (musica.rs).
 * Se pide cuando se va a ver esa música, no al abrir la app. La página
 * pregunta con `DanPlayMusica.tiene()` y lo pide con `DanPlayMusica.pedir()`;
 * la respuesta le llega por `window.__danplayPermisoMusica(concedido)`.
 *
 * Qué canciones hay lo dice el índice de música de Android (MediaStore) con
 * `DanPlayMusica.canciones()`: así salen también las de la tarjeta SD y las
 * de las carpetas de las apps (los audios de Telegram o de WhatsApp, dentro
 * de `Android/media`), que recorriendo carpetas no se veían; y no salen los
 * tonos de llamada, los avisos, las alarmas ni las grabaciones de voz.
 */
class PermisoMusica(private val activity: Activity) {
  @JavascriptInterface
  fun tiene(): Boolean =
    ContextCompat.checkSelfPermission(activity, PERMISO) == PackageManager.PERMISSION_GRANTED

  /**
   * Las canciones del teléfono, en JSON: [{path, title, artist, album,
   * duration (ms), size, mtime}]. Sin el permiso, ninguna.
   */
  @JavascriptInterface
  fun canciones(): String {
    val out = JSONArray()
    if (!tiene()) return out.toString()
    val uri =
      if (Build.VERSION.SDK_INT >= 29) MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
      else MediaStore.Audio.Media.EXTERNAL_CONTENT_URI
    @Suppress("DEPRECATION")
    val ruta = MediaStore.Audio.Media.DATA
    val columnas = arrayOf(
      ruta,
      MediaStore.Audio.Media.TITLE,
      MediaStore.Audio.Media.ARTIST,
      MediaStore.Audio.Media.ALBUM,
      MediaStore.Audio.Media.DURATION,
      MediaStore.Audio.Media.SIZE,
      MediaStore.Audio.Media.DATE_MODIFIED,
    )
    var donde = "${MediaStore.Audio.Media.IS_RINGTONE} = 0 AND " +
      "${MediaStore.Audio.Media.IS_NOTIFICATION} = 0 AND ${MediaStore.Audio.Media.IS_ALARM} = 0"
    if (Build.VERSION.SDK_INT >= 31) donde += " AND ${MediaStore.Audio.Media.IS_RECORDING} = 0"
    try {
      activity.contentResolver.query(uri, columnas, donde, null, null)?.use { c ->
        while (c.moveToNext()) {
          val path = c.getString(0) ?: continue
          out.put(
            JSONObject()
              .put("path", path)
              .put("title", c.getString(1) ?: "")
              .put("artist", c.getString(2) ?: "")
              .put("album", c.getString(3) ?: "")
              .put("duration", c.getLong(4))
              .put("size", c.getLong(5))
              .put("mtime", c.getLong(6))
          )
        }
      }
    } catch (e: Exception) {
      // sin índice (raro): la app recorre las carpetas como antes
      return "null"
    }
    return out.toString()
  }

  @JavascriptInterface
  fun pedir() {
    activity.runOnUiThread {
      ActivityCompat.requestPermissions(activity, arrayOf(PERMISO), CODIGO)
    }
  }

  companion object {
    const val CODIGO = 7401
    val PERMISO: String =
      if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO
      else Manifest.permission.READ_EXTERNAL_STORAGE
  }
}
