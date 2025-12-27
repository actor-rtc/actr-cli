from __future__ import annotations

import argparse
import asyncio
import logging
import sys
import time
from pathlib import Path

from actr import ActrSystem, WorkloadBase

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "generated"))
sys.path.insert(0, str(ROOT))

logging.basicConfig(level=logging.INFO, format="[%(levelname)s] %(message)s")
logger = logging.getLogger(__name__)

from generated import echo_pb2 as pb2
from generated import echo_service_actor as server_actor


class EchoService(server_actor.EchoServiceHandler):
    async def echo(self, req: pb2.EchoRequest, ctx) -> pb2.EchoResponse:
        logger.info("server received: %s", req.message)
        return pb2.EchoResponse(
            reply=f"Echo: {req.message}",
            timestamp=int(time.time()),
        )


class EchoServerWorkload(WorkloadBase):
    def __init__(self, handler: EchoService):
        self.handler = handler
        super().__init__(server_actor.EchoServiceDispatcher())

    async def on_start(self, ctx) -> None:
        logger.info("EchoServerWorkload on_start")

    async def on_stop(self, ctx) -> None:
        logger.info("EchoServerWorkload on_stop")


async def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--actr-toml", required=True)
    args = ap.parse_args()

    system = await ActrSystem.from_toml(args.actr_toml)
    logger.info("Starting Echo server... actr-toml: %s", args.actr_toml)
    workload = EchoServerWorkload(EchoService())
    node = system.attach(workload)
    ref = await node.start()
    logger.info("✅ Echo server started! Actor ID: %s", ref.actor_id())

    await ref.wait_for_ctrl_c_and_shutdown()
    logger.info("Server shutting down...")
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
