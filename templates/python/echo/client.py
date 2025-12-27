from __future__ import annotations

import argparse
import asyncio
import logging
import sys
from pathlib import Path

from actr import ActrSystem, ActrRuntimeError, ActrType, Context, Dest, WorkloadBase

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "generated"))
sys.path.insert(0, str(ROOT))

logging.basicConfig(level=logging.INFO, format="[%(levelname)s] %(message)s")
logger = logging.getLogger(__name__)

from generated import echo_pb2 as pb2


class EchoClientDispatcher:
    async def dispatch(self, workload, route_key: str, payload: bytes, ctx) -> bytes:
        if not isinstance(ctx, Context):
            ctx = Context(ctx)

        if route_key != "echo.EchoService.Echo":
            raise RuntimeError(f"Unknown route_key: {route_key}")

        request = pb2.EchoRequest.FromString(payload)
        await workload.ready.wait()

        if workload.server_id is None:
            response = pb2.EchoResponse(reply="server not available", timestamp=0)
            return response.SerializeToString()

        response_bytes = await ctx.call(
            Dest.actor(workload.server_id),
            "echo.EchoService.Echo",
            request,
        )
        response = pb2.EchoResponse.FromString(response_bytes)
        return response.SerializeToString()


class EchoClientWorkload(WorkloadBase):
    def __init__(self) -> None:
        self.server_type = ActrType("acme", "EchoService")
        self.server_id = None
        self.ready = asyncio.Event()
        super().__init__(EchoClientDispatcher())

    async def on_start(self, ctx) -> None:
        logger.info("discovering echo service...")
        try:
            server_id = await ctx.discover(self.server_type)
        except ActrRuntimeError as e:
            logger.error("discover failed: %s", e)
            self.ready.set()
            return
        self.server_id = server_id
        self.ready.set()
        logger.info("discovered server: %s", server_id)

    async def on_stop(self, ctx) -> None:
        logger.info("EchoClientWorkload on_stop")


async def _run_app(ref) -> None:
    logger.info("Echo client app started")
    print("===== Echo Client App =====")
    print("Type messages to send to server (type 'quit' to exit):")

    loop = asyncio.get_running_loop()
    while True:
        line = await loop.run_in_executor(None, lambda: input("> "))
        line = line.strip()
        if line in {"quit", "exit"}:
            break
        if not line:
            continue

        request = pb2.EchoRequest(message=line)
        try:
            response_bytes = await ref.call("echo.EchoService.Echo", request)
            response = pb2.EchoResponse.FromString(response_bytes)
            print(f"[Received reply] {response.reply}")
        except Exception as e:
            logger.error("app call failed: %s", e)
            print(f"[Error] {e}")


async def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--actr-toml", required=True)
    args = ap.parse_args()

    system = await ActrSystem.from_toml(args.actr_toml)
    workload = EchoClientWorkload()
    node = system.attach(workload)
    ref = await node.start()
    logger.info("✅ Echo client started! Actor ID: %s", ref.actor_id())

    await _run_app(ref)
    ref.shutdown()
    await ref.wait_for_shutdown()
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
