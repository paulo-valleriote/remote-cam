package remotecam.app

import android.app.Application
import android.util.Log

/**
 * RemoteCam Application class.
 * Initializes WebRTC and other global state.
 */
class RemoteCamApplication : Application() {

    override fun onCreate() {
        super.onCreate()
        // WebRTC initialization is done lazily in PeerConnectionFactoryProvider
        Log.d("RemoteCam", "Application created")
    }
}