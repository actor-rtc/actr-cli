package {{PACKAGE_NAME}}

import android.content.Context
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * ActrService - Actor-RTC Service Stub
 *
 * This is a placeholder service. To complete the implementation:
 *
 * 1. Install service dependencies to get proto files:
 *    ```
 *    actr install
 *    ```
 *
 * 2. Generate the Echo client code:
 *    ```
 *    actr gen -l kotlin -i protos/remote/{{service-name}}/{{proto-file}} -o app/src/main/java/{{PACKAGE_PATH}}/generated
 *    ```
 *
 * 3. Copy the actr-kotlin library to your project:
 *    - Option A: Copy actr-kotlin.aar to app/libs/ and add `implementation(files("libs/actr-kotlin.aar"))`
 *    - Option B: Include actr-kotlin as a module dependency
 *
 * 4. Replace this stub with the generated ActrService implementation
 *
 * The generated code will provide:
 * - EchoServiceDispatcher for handling incoming requests
 * - EchoServiceWorkload for actor lifecycle management
 * - Helper methods for RPC calls
 */
class ActrService(private val context: Context) {

    enum class ConnectionStatus {
        DISCONNECTED,
        CONNECTING,
        CONNECTED,
        ERROR;

        val description: String
            get() = when (this) {
                DISCONNECTED -> "Disconnected"
                CONNECTING -> "Connecting"
                CONNECTED -> "Connected"
                ERROR -> "Error"
            }
    }

    var connectionStatus: ConnectionStatus = ConnectionStatus.DISCONNECTED
        private set

    var errorMessage: String? = null
        private set

    /**
     * Initialize and start the Actor-RTC connection
     */
    suspend fun start() {
        connectionStatus = ConnectionStatus.ERROR
        errorMessage = STUB_ERROR_MESSAGE
        throw ActrServiceException(STUB_ERROR_MESSAGE)
    }

    /**
     * Send an echo request to the server
     */
    suspend fun echo(message: String): String {
        errorMessage = STUB_ERROR_MESSAGE
        throw ActrServiceException(STUB_ERROR_MESSAGE)
    }

    /**
     * Stop and clean up the Actor-RTC connection
     */
    suspend fun stop() {
        withContext(Dispatchers.IO) {
            connectionStatus = ConnectionStatus.DISCONNECTED
            errorMessage = null
        }
    }

    companion object {
        private const val STUB_ERROR_MESSAGE = 
            "ActrService is not generated. First run 'actr install', then:\n" +
            "actr gen -l kotlin -i protos/remote/{{service-name}}/{{proto-file}} -o app/src/main/java/{{PACKAGE_PATH}}/generated"
    }
}

class ActrServiceException(message: String) : Exception(message)
