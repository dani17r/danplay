package local.danplay.pistas

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.graphics.Color
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject

/**
 * «Abrir con DanPlay»: un .danpista tocado en WhatsApp, en Descargas
 * o en Drive llega aqui como un `content://`, y se le pasa a la pagina, que
 * lo importa igual que con el boton «Importar» (src/movil/importar.js).
 *
 * La pagina puede no estar lista todavia (la app se abre con el archivo), asi
 * que se le pregunta si ya tiene con que recibirlo (`__danplayAbrir`) y se
 * vuelve a probar al rato, hasta que lo coja.
 */
class MainActivity : TauriActivity() {
  private var webView: WebView? = null
  private val pending = ArrayDeque<String>()
  private val main = Handler(Looper.getMainLooper())

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Desde Android 15 la app se pinta por debajo de las barras del sistema:
    // la hora tapaba el buscador y la barra de gestos el boton de sonar. El
    // contenido se aparta lo que ocupan, y ese hueco lleva el fondo de DanPlay
    // con los iconos del sistema en claro.
    window.decorView.setBackgroundColor(Color.parseColor("#0c0d10"))
    WindowCompat.getInsetsController(window, window.decorView).apply {
      isAppearanceLightStatusBars = false
      isAppearanceLightNavigationBars = false
    }
    val content = findViewById<android.view.View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    opened(intent)?.let { pending.add(it) }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    opened(intent)?.let {
      pending.add(it)
      deliver()
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    // la página le cuenta a Android qué suena, para la notificación con los
    // mandos y la pantalla de bloqueo (Reproduciendo.kt)
    webView.addJavascriptInterface(Puente(applicationContext), "DanPlayAndroid")
    // «Atras»: primero la pagina cierra lo que tenga abierto (un menu, la
    // ficha, el estudio); si no tenia nada, la app pasa a segundo plano y la
    // musica sigue sonando, como en cualquier reproductor. Se registra aqui,
    // despues de los plugins de Tauri, para que sea la que responde.
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        webView.evaluateJavascript("window.__danplayAtras ? window.__danplayAtras() : false") { closed ->
          if (closed != "true") moveTaskToBack(true)
        }
      }
    })
    deliver()
  }

  /** El archivo con que se abrio la app, si se abrio con uno. */
  private fun opened(intent: Intent?): String? {
    if (intent == null) return null
    val uri: Uri? = when (intent.action) {
      Intent.ACTION_VIEW -> intent.data
      Intent.ACTION_SEND ->
        if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        else @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_STREAM)
      else -> null
    }
    return uri?.toString()
  }

  private fun deliver(tries: Int = 0) {
    val view = webView ?: return
    val uri = pending.firstOrNull() ?: return
    val js = "typeof window.__danplayAbrir === 'function' ? (window.__danplayAbrir(${JSONObject.quote(uri)}), 'ok') : 'no'"
    view.post {
      view.evaluateJavascript(js) { result ->
        if (result == "\"ok\"") {
          pending.removeFirstOrNull()
          deliver()
        } else if (tries < 100) {
          main.postDelayed({ deliver(tries + 1) }, 300)
        }
      }
    }
  }
}
