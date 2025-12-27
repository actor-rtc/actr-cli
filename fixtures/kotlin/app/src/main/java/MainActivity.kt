package {{PACKAGE_NAME}}

import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import {{PACKAGE_NAME}}.R

/**
 * {{PROJECT_NAME_PASCAL}} Echo Client Main Activity
 *
 * This activity provides a simple UI to:
 * 1. Connect to the Echo server via Actor-RTC
 * 2. Send messages and receive echo responses
 * 3. Display connection status and message logs
 *
 * To complete the implementation:
 * 1. Run `actr gen -l kotlin -i protos/echo.proto -o app/src/main/java/{{PACKAGE_PATH}}/generated`
 * 2. Copy actr-kotlin library (AAR or source) to the project
 * 3. Implement ActrService.kt with the generated code
 */
class MainActivity : AppCompatActivity() {

    private lateinit var statusText: TextView
    private lateinit var connectButton: Button
    private lateinit var disconnectButton: Button
    private lateinit var messageInput: EditText
    private lateinit var sendButton: Button
    private lateinit var logText: TextView
    private lateinit var scrollView: ScrollView

    // TODO: Initialize ActrService
    // private lateinit var actrService: ActrService

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        initViews()
        setupClickListeners()
        
        log("Ready to connect to Echo server")
        log("Signaling URL: {{SIGNALING_URL}}")
    }

    private fun initViews() {
        statusText = findViewById(R.id.statusText)
        connectButton = findViewById(R.id.connectButton)
        disconnectButton = findViewById(R.id.disconnectButton)
        messageInput = findViewById(R.id.messageInput)
        sendButton = findViewById(R.id.sendButton)
        logText = findViewById(R.id.logText)
        scrollView = findViewById(R.id.scrollView)
    }

    private fun setupClickListeners() {
        connectButton.setOnClickListener {
            connect()
        }

        disconnectButton.setOnClickListener {
            disconnect()
        }

        sendButton.setOnClickListener {
            sendMessage()
        }
    }

    private fun connect() {
        updateStatus("Connecting...")
        connectButton.isEnabled = false
        
        lifecycleScope.launch {
            try {
                // TODO: Initialize and start ActrService
                // actrService = ActrService(applicationContext)
                // actrService.start()
                
                withContext(Dispatchers.Main) {
                    updateStatus("Connected")
                    disconnectButton.isEnabled = true
                    messageInput.isEnabled = true
                    sendButton.isEnabled = true
                    log("Connected to Echo server")
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    updateStatus("Connection failed")
                    connectButton.isEnabled = true
                    log("Error: ${e.message}")
                }
            }
        }
    }

    private fun disconnect() {
        updateStatus("Disconnecting...")
        disconnectButton.isEnabled = false
        messageInput.isEnabled = false
        sendButton.isEnabled = false
        
        lifecycleScope.launch {
            try {
                // TODO: Stop ActrService
                // actrService.stop()
                
                withContext(Dispatchers.Main) {
                    updateStatus("Disconnected")
                    connectButton.isEnabled = true
                    log("Disconnected from Echo server")
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    updateStatus("Disconnected")
                    connectButton.isEnabled = true
                    log("Disconnect error: ${e.message}")
                }
            }
        }
    }

    private fun sendMessage() {
        val message = messageInput.text.toString().trim()
        if (message.isEmpty()) return

        messageInput.text.clear()
        log("Sending: $message")

        lifecycleScope.launch {
            try {
                // TODO: Send echo request
                // val response = actrService.echo(message)
                // log("Received: ${response.reply}")
                
                // Placeholder until ActrService is implemented
                withContext(Dispatchers.Main) {
                    log("Echo: $message (stub - implement ActrService)")
                }
            } catch (e: Exception) {
                withContext(Dispatchers.Main) {
                    log("Send error: ${e.message}")
                }
            }
        }
    }

    private fun updateStatus(status: String) {
        statusText.text = "Status: $status"
    }

    private fun log(message: String) {
        val timestamp = java.text.SimpleDateFormat("HH:mm:ss", java.util.Locale.getDefault())
            .format(java.util.Date())
        val logEntry = "[$timestamp] $message\n"
        logText.append(logEntry)
        scrollView.post { scrollView.fullScroll(ScrollView.FOCUS_DOWN) }
    }

    override fun onDestroy() {
        super.onDestroy()
        // TODO: Clean up ActrService
        // if (::actrService.isInitialized) {
        //     actrService.stop()
        // }
    }
}
