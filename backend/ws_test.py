import asyncio
import json
import websockets
import uuid

async def test_ws():
    token = "e0392a23-25f4-47b3-86c2-2a0df1070e6e"
    uri = f"ws://localhost:3000/ws?token={token}"
    async with websockets.connect(uri) as websocket:
        print("Connected")

        # Wait for server message
        message = await websocket.recv()
        print(f"Received: {message}")

        # Send confirmation
        deployment_id = "1137ae55-7444-49a4-be3d-840d396aad17"
        confirm = {"type": "update_confirmed", "deployment_id": deployment_id}
        await websocket.send(json.dumps(confirm))
        print(f"Sent update_confirmed for deployment {deployment_id}")

        # Wait a bit for server to process
        await asyncio.sleep(2)

    print("Disconnected")

if __name__ == "__main__":
    asyncio.run(test_ws())