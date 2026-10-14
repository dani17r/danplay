package local.danplay.pistas

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.webkit.JavascriptInterface
import org.json.JSONObject
import java.io.File

/**
 * Los mandos del reproductor fuera de la app: la notificación de la bandeja
 * (con pausa, anterior, siguiente y la barra de tiempo), la pantalla de
 * bloqueo, los auriculares y el reloj.
 *
 * Es una sesión multimedia de Android dentro de un servicio «en primer plano»
 * de reproducción: así Android sabe que la app está sonando, dibuja sus
 * mandos y no la cierra en segundo plano. Mientras suena, la notificación no
 * se puede quitar; en pausa, sí.
 *
 * La página le cuenta qué suena (`DanPlayAndroid.update`, ver Puente y
 * src/movil/mediaSession.js). Lo que se pulsa va directo al motor en Rust
 * (`mandoNativo`, movil/src/mandos.rs), sin pasar por la página, que se
 * entera por el evento de siempre y vuelve a contar cómo quedó.
 */
class Reproduciendo : Service() {
  /** Lo que suena, tal como lo cuenta la página. */
  data class Ahora(
    val id: Long,
    val title: String,
    val artist: String,
    val folder: String,
    val duration: Long,
    val position: Long,
    val speed: Float,
    val playing: Boolean,
    val hasNext: Boolean,
  ) {
    companion object {
      /** null: no suena nada (se quita la notificación). */
      fun de(json: String): Ahora? {
        val o = try { JSONObject(json) } catch (e: Exception) { return null }
        if (o.isNull("id")) return null
        return Ahora(
          id = o.optLong("id"),
          title = o.optString("title"),
          artist = o.optString("artist"),
          folder = o.optString("folder"),
          duration = o.optLong("duration"),
          position = o.optLong("position"),
          speed = o.optDouble("speed", 1.0).toFloat(),
          playing = o.optBoolean("playing"),
          hasNext = o.optBoolean("hasNext", true),
        )
      }
    }
  }

  companion object {
    // los mismos números que movil/src/mandos.rs
    const val PLAY = 0
    const val PAUSE = 1
    const val TOGGLE = 2
    const val NEXT = 3
    const val PREVIOUS = 4
    const val SEEK = 5

    private const val CANAL = "reproduccion"
    private const val AVISO = 1
    private const val EXTRA = "mando"
    private val main = Handler(Looper.getMainLooper())

    @Volatile private var ahora: Ahora? = null
    private var servicio: Reproduciendo? = null

    /** En Rust: el reproductor cumple lo que se pulsó. */
    @JvmStatic external fun mandoNativo(accion: Int, segundos: Double): Boolean

    fun mando(accion: Int, segundos: Double = 0.0) {
      try {
        mandoNativo(accion, segundos)
      } catch (e: UnsatisfiedLinkError) {
        // la app no está cargada (no debería pasar con la notificación puesta)
      }
    }

    /** La página cuenta qué suena. Llega en el hilo del WebView. */
    fun cuenta(context: Context, json: String) {
      val nuevo = Ahora.de(json)
      main.post {
        ahora = nuevo
        val s = servicio
        when {
          nuevo == null -> s?.parar()
          s != null -> s.pintar(nuevo)
          // arranca al ponerse a sonar, con la app delante (se acaba de tocar)
          nuevo.playing -> try {
            context.startForegroundService(Intent(context, Reproduciendo::class.java))
          } catch (e: Exception) {
            // Android no deja arrancarlo desde segundo plano: a la próxima
          }
        }
      }
    }
  }

  private lateinit var session: MediaSession
  private var portadaDe: String? = null
  private var portada: Bitmap? = null
  private var enPrimerPlano = false

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    servicio = this
    val nm = getSystemService(NotificationManager::class.java)
    nm.createNotificationChannel(
      NotificationChannel(CANAL, "Reproducción", NotificationManager.IMPORTANCE_LOW).apply {
        description = "Los mandos de lo que suena: pausa, anterior y siguiente"
        setShowBadge(false)
      }
    )
    session = MediaSession(this, "DanPlay").apply {
      setCallback(object : MediaSession.Callback() {
        override fun onPlay() = mando(PLAY)
        override fun onPause() = mando(PAUSE)
        override fun onStop() = mando(PAUSE)
        override fun onSkipToNext() = mando(NEXT)
        override fun onSkipToPrevious() = mando(PREVIOUS)
        override fun onSeekTo(pos: Long) = mando(SEEK, pos / 1000.0)
      })
      setSessionActivity(abrirApp())
      isActive = true
    }
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    // los botones de la notificación (Android 8 a 12; desde el 13 la dibuja
    // el sistema con la sesión y llegan por el Callback de arriba)
    when (intent?.getIntExtra(EXTRA, -1) ?: -1) {
      -1 -> {}
      else -> mando(intent!!.getIntExtra(EXTRA, TOGGLE))
    }
    val a = ahora
    if (a != null) pintar(a) else parar()
    return START_NOT_STICKY
  }

  /** Quitar la app de recientes: se para la música y se va la notificación. */
  override fun onTaskRemoved(rootIntent: Intent?) {
    mando(PAUSE)
    parar()
    super.onTaskRemoved(rootIntent)
  }

  override fun onDestroy() {
    session.isActive = false
    session.release()
    servicio = null
    super.onDestroy()
  }

  fun pintar(a: Ahora) {
    session.setMetadata(
      MediaMetadata.Builder()
        .putString(MediaMetadata.METADATA_KEY_TITLE, a.title)
        .putString(MediaMetadata.METADATA_KEY_ARTIST, a.artist)
        .putLong(MediaMetadata.METADATA_KEY_DURATION, a.duration)
        .apply { portada(a.folder)?.let { putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, it) } }
        .build()
    )
    var acciones = PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or
      PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_SKIP_TO_PREVIOUS or
      PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_STOP
    if (a.hasNext) acciones = acciones or PlaybackState.ACTION_SKIP_TO_NEXT
    session.setPlaybackState(
      PlaybackState.Builder()
        .setActions(acciones)
        .setState(
          if (a.playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
          a.position,
          if (a.playing) a.speed else 0f
        )
        .build()
    )
    val aviso = notificacion(a)
    val nm = getSystemService(NotificationManager::class.java)
    if (a.playing) {
      try {
        if (Build.VERSION.SDK_INT >= 29) {
          startForeground(AVISO, aviso, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        } else {
          startForeground(AVISO, aviso)
        }
        enPrimerPlano = true
      } catch (e: Exception) {
        nm.notify(AVISO, aviso)
      }
    } else {
      if (enPrimerPlano) {
        stopForeground(STOP_FOREGROUND_DETACH)
        enPrimerPlano = false
      }
      nm.notify(AVISO, aviso)
    }
  }

  fun parar() {
    stopForeground(STOP_FOREGROUND_REMOVE)
    enPrimerPlano = false
    getSystemService(NotificationManager::class.java).cancel(AVISO)
    stopSelf()
  }

  private fun notificacion(a: Ahora): Notification {
    val boton = { icono: Int, nombre: String, accion: Int ->
      Notification.Action.Builder(
        Icon.createWithResource(this, icono),
        nombre,
        PendingIntent.getService(
          this,
          accion,
          Intent(this, Reproduciendo::class.java).putExtra(EXTRA, accion),
          PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
      ).build()
    }
    return Notification.Builder(this, CANAL)
      .setSmallIcon(R.drawable.ic_stat_danplay)
      .setContentTitle(a.title)
      .setContentText(a.artist)
      .setLargeIcon(portada(a.folder))
      .setContentIntent(abrirApp())
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      .setCategory(Notification.CATEGORY_TRANSPORT)
      .setOngoing(a.playing)
      .setShowWhen(false)
      .addAction(boton(R.drawable.ic_mando_anterior, "Anterior", PREVIOUS))
      .addAction(
        if (a.playing) boton(R.drawable.ic_mando_pausa, "Pausar", PAUSE)
        else boton(R.drawable.ic_mando_sonar, "Reproducir", PLAY)
      )
      .addAction(boton(R.drawable.ic_mando_siguiente, "Siguiente", NEXT))
      .setStyle(
        Notification.MediaStyle()
          .setMediaSession(session.sessionToken)
          .setShowActionsInCompactView(0, 1, 2)
      )
      .build()
  }

  /** Tocar la notificación abre la app donde estaba. */
  private fun abrirApp(): PendingIntent = PendingIntent.getActivity(
    this,
    0,
    Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
  )

  /** La portada de la pista (la que nombra su `pista.json`), pequeña. */
  private fun portada(folder: String): Bitmap? {
    if (folder == portadaDe) return portada
    portadaDe = folder
    portada = try {
      val dir = File(folder)
      val nombre = JSONObject(File(dir, "pista.json").readText()).optString("cover")
      val archivo = File(dir, nombre)
      if (nombre.isEmpty() || !archivo.isFile) null else {
        val medidas = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(archivo.path, medidas)
        var escala = 1
        while (medidas.outWidth / (escala * 2) >= 512) escala *= 2
        BitmapFactory.decodeFile(archivo.path, BitmapFactory.Options().apply { inSampleSize = escala })
      }
    } catch (e: Exception) {
      null
    }
    return portada
  }
}

/**
 * Lo que la página puede decirle a Android: `DanPlayAndroid.update(json)`
 * con lo que suena. Lo pone MainActivity en el WebView.
 */
class Puente(private val context: Context) {
  @JavascriptInterface
  fun update(json: String) {
    Reproduciendo.cuenta(context, json)
  }
}
