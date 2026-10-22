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
 * La página le cuenta qué suena (`DanPlayAndroid.update`, ver AndroidBridge y
 * src/mobile/mediaSession.js). Lo que se pulsa va directo al motor en Rust
 * (`nativePress`, mobile/src/controls.rs), sin pasar por la página, que se
 * entera por el evento de siempre y vuelve a contar cómo quedó.
 */
class PlaybackService : Service() {
  /** Lo que suena, tal como lo cuenta la página. */
  data class NowPlaying(
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
      fun parse(json: String): NowPlaying? {
        val o = try { JSONObject(json) } catch (e: Exception) { return null }
        if (o.isNull("id")) return null
        return NowPlaying(
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
    // los mismos números que mobile/src/controls.rs
    const val PLAY = 0
    const val PAUSE = 1
    const val TOGGLE = 2
    const val NEXT = 3
    const val PREVIOUS = 4
    const val SEEK = 5

    private const val CHANNEL = "reproduccion"
    private const val NOTIFICATION_ID = 1
    private const val EXTRA = "press"
    private val main = Handler(Looper.getMainLooper())

    @Volatile private var nowPlaying: NowPlaying? = null
    private var instance: PlaybackService? = null

    /** En Rust: el reproductor cumple lo que se pulsó. */
    @JvmStatic external fun nativePress(action: Int, seconds: Double): Boolean

    fun press(action: Int, seconds: Double = 0.0) {
      try {
        nativePress(action, seconds)
      } catch (e: UnsatisfiedLinkError) {
        // la app no está cargada (no debería pasar con la notificación puesta)
      }
    }

    /** La página cuenta qué suena. Llega en el hilo del WebView. */
    fun report(context: Context, json: String) {
      val latest = NowPlaying.parse(json)
      main.post {
        nowPlaying = latest
        val s = instance
        when {
          latest == null -> s?.dismiss()
          s != null -> s.show(latest)
          // arranca al ponerse a sonar, con la app delante (se acaba de tocar)
          latest.playing -> try {
            context.startForegroundService(Intent(context, PlaybackService::class.java))
          } catch (e: Exception) {
            // Android no deja arrancarlo desde segundo plano: a la próxima
          }
        }
      }
    }
  }

  private lateinit var session: MediaSession
  private var coverFolder: String? = null
  private var cover: Bitmap? = null
  private var inForeground = false

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    instance = this
    val nm = getSystemService(NotificationManager::class.java)
    nm.createNotificationChannel(
      NotificationChannel(CHANNEL, "Reproducción", NotificationManager.IMPORTANCE_LOW).apply {
        description = "Los mandos de lo que suena: pausa, anterior y siguiente"
        setShowBadge(false)
      }
    )
    session = MediaSession(this, "DanPlay").apply {
      setCallback(object : MediaSession.Callback() {
        override fun onPlay() = press(PLAY)
        override fun onPause() = press(PAUSE)
        override fun onStop() = press(PAUSE)
        override fun onSkipToNext() = press(NEXT)
        override fun onSkipToPrevious() = press(PREVIOUS)
        override fun onSeekTo(pos: Long) = press(SEEK, pos / 1000.0)
      })
      setSessionActivity(openApp())
      isActive = true
    }
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    // los botones de la notificación (Android 8 a 12; desde el 13 la dibuja
    // el sistema con la sesión y llegan por el Callback de arriba)
    when (intent?.getIntExtra(EXTRA, -1) ?: -1) {
      -1 -> {}
      else -> press(intent!!.getIntExtra(EXTRA, TOGGLE))
    }
    val a = nowPlaying
    if (a != null) show(a) else dismiss()
    return START_NOT_STICKY
  }

  /** Quitar la app de recientes: se para la música y se va la notificación. */
  override fun onTaskRemoved(rootIntent: Intent?) {
    press(PAUSE)
    dismiss()
    super.onTaskRemoved(rootIntent)
  }

  override fun onDestroy() {
    session.isActive = false
    session.release()
    instance = null
    super.onDestroy()
  }

  fun show(a: NowPlaying) {
    session.setMetadata(
      MediaMetadata.Builder()
        .putString(MediaMetadata.METADATA_KEY_TITLE, a.title)
        .putString(MediaMetadata.METADATA_KEY_ARTIST, a.artist)
        .putLong(MediaMetadata.METADATA_KEY_DURATION, a.duration)
        .apply { coverOf(a.folder)?.let { putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, it) } }
        .build()
    )
    var actions = PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or
      PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_SKIP_TO_PREVIOUS or
      PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_STOP
    if (a.hasNext) actions = actions or PlaybackState.ACTION_SKIP_TO_NEXT
    session.setPlaybackState(
      PlaybackState.Builder()
        .setActions(actions)
        .setState(
          if (a.playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
          a.position,
          if (a.playing) a.speed else 0f
        )
        .build()
    )
    val notification = buildNotification(a)
    val nm = getSystemService(NotificationManager::class.java)
    if (a.playing) {
      try {
        if (Build.VERSION.SDK_INT >= 29) {
          startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
        } else {
          startForeground(NOTIFICATION_ID, notification)
        }
        inForeground = true
      } catch (e: Exception) {
        nm.notify(NOTIFICATION_ID, notification)
      }
    } else {
      if (inForeground) {
        stopForeground(STOP_FOREGROUND_DETACH)
        inForeground = false
      }
      nm.notify(NOTIFICATION_ID, notification)
    }
  }

  fun dismiss() {
    stopForeground(STOP_FOREGROUND_REMOVE)
    inForeground = false
    getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
    stopSelf()
  }

  private fun buildNotification(a: NowPlaying): Notification {
    val button = { icon: Int, name: String, action: Int ->
      Notification.Action.Builder(
        Icon.createWithResource(this, icon),
        name,
        PendingIntent.getService(
          this,
          action,
          Intent(this, PlaybackService::class.java).putExtra(EXTRA, action),
          PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
      ).build()
    }
    return Notification.Builder(this, CHANNEL)
      .setSmallIcon(R.drawable.ic_stat_danplay)
      .setContentTitle(a.title)
      .setContentText(a.artist)
      .setLargeIcon(coverOf(a.folder))
      .setContentIntent(openApp())
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      .setCategory(Notification.CATEGORY_TRANSPORT)
      .setOngoing(a.playing)
      .setShowWhen(false)
      .addAction(button(R.drawable.ic_control_previous, "Anterior", PREVIOUS))
      .addAction(
        if (a.playing) button(R.drawable.ic_control_pause, "Pausar", PAUSE)
        else button(R.drawable.ic_control_play, "Reproducir", PLAY)
      )
      .addAction(button(R.drawable.ic_control_next, "Siguiente", NEXT))
      .setStyle(
        Notification.MediaStyle()
          .setMediaSession(session.sessionToken)
          .setShowActionsInCompactView(0, 1, 2)
      )
      .build()
  }

  /** Tocar la notificación abre la app donde estaba. */
  private fun openApp(): PendingIntent = PendingIntent.getActivity(
    this,
    0,
    Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
  )

  /** La portada de la pista (la que nombra su `pista.json`), pequeña. */
  private fun coverOf(folder: String): Bitmap? {
    if (folder == coverFolder) return cover
    coverFolder = folder
    cover = try {
      val dir = File(folder)
      val name = JSONObject(File(dir, "pista.json").readText()).optString("cover")
      val file = File(dir, name)
      if (name.isEmpty() || !file.isFile) null else {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        var scale = 1
        while (bounds.outWidth / (scale * 2) >= 512) scale *= 2
        BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = scale })
      }
    } catch (e: Exception) {
      null
    }
    return cover
  }
}

/**
 * Lo que la página puede decirle a Android: `DanPlayAndroid.update(json)`
 * con lo que suena. Lo pone MainActivity en el WebView.
 */
class AndroidBridge(private val context: Context) {
  @JavascriptInterface
  fun update(json: String) {
    PlaybackService.report(context, json)
  }
}
