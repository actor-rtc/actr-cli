package {{PACKAGE_NAME}}

import android.content.Context
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import {{PACKAGE_NAME}}.generated.EchoServiceHandler
import {{PACKAGE_NAME}}.generated.EchoServiceDispatcher
import echo.Echo.EchoRequest
import echo.Echo.EchoResponse
import io.actor_rtc.actr.ActrId
import io.actor_rtc.actr.ActrType
import io.actor_rtc.actr.ContextBridge
import io.actor_rtc.actr.PayloadType
import io.actor_rtc.actr.Realm
import io.actor_rtc.actr.RpcEnvelopeBridge
import io.actor_rtc.actr.WorkloadBridge
import io.actor_rtc.actr.dsl.*
import java.io.File
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

/**
 * {{PROJECT_NAME_PASCAL}} Echo Integration Test
 *
 * This test verifies the RPC call to the remote EchoService.
 * Make sure the EchoService server is running before executing this test.
 *
 * This test demonstrates using generated Handler and Dispatcher code
 * from protoc-gen-actrframework-kotlin.
 */
@RunWith(AndroidJUnit4::class)
class EchoIntegrationTest {

    companion object {
        private const val TAG = "EchoIntegrationTest"
        private const val REALM_ID = 2281844430u
    }

    private fun getContext(): Context {
        return InstrumentationRegistry.getInstrumentation().targetContext
    }

    private fun copyAssetToInternalStorage(assetName: String): String {
        val context = getContext()
        val inputStream = context.assets.open(assetName)
        val outputFile = File(context.filesDir, assetName)
        outputFile.parentFile?.mkdirs()
        inputStream.use { input ->
            outputFile.outputStream().use { output -> input.copyTo(output) }
        }
        return outputFile.absolutePath
    }

    // ==================== Client Handler Implementation ====================

    /**
     * Client-side handler that forwards requests to the remote EchoService.
     * This demonstrates using the generated Handler interface for client-side logic.
     */
    private inner class EchoClientHandler(
        private val ctx: ContextBridge,
        private val serverId: ActrId
    ) : EchoServiceHandler {

        override suspend fun echo(request: EchoRequest, ctx: ContextBridge): EchoResponse {
            Log.i(TAG, "📤 Forwarding echo request to server: ${request.message}")
            
            // Forward to remote EchoService
            val response = ctx.callRaw(
                serverId,
                "echo.EchoService.Echo",
                PayloadType.RPC_RELIABLE,
                request.toByteArray(),
                30000L
            )
            
            val echoResponse = EchoResponse.parseFrom(response)
            Log.i(TAG, "📥 Received response from server: ${echoResponse.reply}")
            return echoResponse
        }
    }

    // ==================== Client Workload ====================

    /**
     * Workload that handles RPC requests using the generated Dispatcher.
     *
     * This demonstrates the proper pattern:
     * 1. Pre-discover the EchoService in onStart
     * 2. Use EchoServiceDispatcher to route requests to handler
     * 3. Handler forwards to remote server
     */
    private inner class EchoClientWorkload : WorkloadBridge {

        // Server ID discovered in onStart
        private var echoServerId: ActrId? = null
        private var handler: EchoClientHandler? = null

        override suspend fun onStart(ctx: ContextBridge) {
            Log.i(TAG, "EchoClientWorkload.onStart: Starting...")

            // Pre-discover the EchoService
            Log.i(TAG, "📡 Discovering EchoService...")
            val targetType = ActrType(manufacturer = "acme", name = "EchoService")
            echoServerId = ctx.discover(targetType)
            Log.i(TAG, "✅ Found EchoService: ${echoServerId?.serialNumber}")

            // Create handler with discovered server ID
            handler = EchoClientHandler(ctx, echoServerId!!)
        }

        override suspend fun onStop(ctx: ContextBridge) {
            Log.i(TAG, "EchoClientWorkload.onStop")
        }

        /**
         * Dispatch RPC requests using the generated Dispatcher
         */
        override suspend fun dispatch(ctx: ContextBridge, envelope: RpcEnvelopeBridge): ByteArray {
            Log.i(TAG, "🔀 EchoClientWorkload.dispatch() called!")
            Log.i(TAG, "   route_key: ${envelope.routeKey}")
            Log.i(TAG, "   request_id: ${envelope.requestId}")
            Log.i(TAG, "   payload size: ${envelope.payload.size} bytes")

            val currentHandler = handler 
                ?: throw IllegalStateException("Handler not initialized - EchoService not discovered yet")

            // Use generated Dispatcher to route to handler
            return EchoServiceDispatcher.dispatch(currentHandler, ctx, envelope)
        }
    }

    /**
     * Test RPC call to remote EchoService using generated code
     *
     * This test:
     * 1. Creates a client workload that uses generated Dispatcher
     * 2. Sends an echo request via ActrRef.call()
     * 3. Verifies the response matches expected format
     *
     * Prerequisites:
     * - EchoService server must be running
     * - Signaling server must be accessible
     */
    @Test
    fun testRpcCallToEchoServer(): Unit = runBlocking {
        Log.i(TAG, "=== Starting RPC Call Test (using generated code) ===")
        val clientConfigPath = copyAssetToInternalStorage("actr-config.toml")
        var clientRef: ActrRef? = null

        try {
            val clientSystem = createActrSystem(clientConfigPath)
            val testMessage = "Hello from {{PROJECT_NAME_PASCAL}}!"
            val expectedResponse = "Echo: $testMessage"

            val clientWorkload = EchoClientWorkload()
            val clientNode = clientSystem.attach(clientWorkload)
            clientRef = clientNode.start()
            Log.i(TAG, "Client started: ${clientRef.actorId().serialNumber}")

            // Wait for onStart to complete (which discovers the server)
            delay(2000)

            // Create EchoRequest using generated protobuf class
            val request = EchoRequest.newBuilder()
                .setMessage(testMessage)
                .build()

            // Send RPC via ActrRef.call() - this triggers the dispatch() method
            Log.i(TAG, "📞 Sending RPC via ActrRef.call()...")
            val responsePayload = clientRef.call(
                "echo.EchoService.Echo",
                PayloadType.RPC_RELIABLE,
                request.toByteArray(),
                30000L
            )

            // Parse response using generated protobuf class
            val response = EchoResponse.parseFrom(responsePayload)
            Log.i(TAG, "📬 Response: ${response.reply}")

            assertEquals("Echo mismatch", expectedResponse, response.reply)
            Log.i(TAG, "=== RPC Call Test PASSED ===")
        } finally {
            try {
                clientRef?.shutdown()
                clientRef?.awaitShutdown()
            } catch (e: Exception) {
                Log.w(TAG, "Error during cleanup: ${e.message}")
            }
        }
    }
}
