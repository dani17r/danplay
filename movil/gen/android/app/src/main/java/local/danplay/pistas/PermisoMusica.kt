package local.danplay.pistas

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import android.webkit.JavascriptInterface
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

/**
 * La música del teléfono (src/movil, «Música del teléfono»): el permiso de
 * leer «música y audio» para abrir las canciones con su ruta (musica.rs).
 * Se pide cuando se va a ver esa música, no al abrir la app. La página
 * pregunta con `DanPlayMusica.tiene()` y lo pide con `DanPlayMusica.pedir()`;
 * la respuesta le llega por `window.__danplayPermisoMusica(concedido)`.
 */
class PermisoMusica(private val activity: Activity) {
  @JavascriptInterface
  fun tiene(): Boolean =
    ContextCompat.checkSelfPermission(activity, PERMISO) == PackageManager.PERMISSION_GRANTED

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
