#!/bin/bash

set -e

BASE_URL="${BASE_URL:-http://localhost:3000}"
API_URL="$BASE_URL/api"

echo "=== OTAedge API Test Suite ==="
echo "Base URL: $BASE_URL"

# Test 1: Health check
echo -e "\n[TEST 1] Health check..."
curl -s "$BASE_URL/health" | grep -q "ok" && echo "✅ PASS" || echo "❌ FAIL"

# Test 2: Register user
echo -e "\n[TEST 2] Register user..."
REGISTER_RESP=$(curl -s -X POST "$API_URL/auth/register" \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","username":"testuser","password":"TestPass123"}')
echo "$REGISTER_RESP" | grep -q "token" && echo "✅ PASS" || echo "❌ FAIL - Response: $REGISTER_RESP"

# Extract token
TOKEN=$(echo "$REGISTER_RESP" | grep -o '"token":"[^"]*"' | cut -d'"' -f4)
if [ -z "$TOKEN" ]; then
  echo "❌ Could not extract token, trying login..."
  LOGIN_RESP=$(curl -s -X POST "$API_URL/auth/login" \
    -H "Content-Type: application/json" \
    -d '{"email":"test@example.com","password":"TestPass123"}')
  TOKEN=$(echo "$LOGIN_RESP" | grep -o '"token":"[^"]*"' | cut -d'"' -f4)
fi

if [ -z "$TOKEN" ]; then
  echo "❌ Failed to get auth token"
  exit 1
fi

echo "✅ Got auth token"

# Test 3: Create device
echo -e "\n[TEST 3] Create device..."
DEVICE_RESP=$(curl -s -X POST "$API_URL/devices" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"device_id":"rpi-test-001","name":"Test Raspberry Pi","device_type":"raspberry_pi_4"}')
echo "$DEVICE_RESP" | grep -q "id" && echo "✅ PASS" || echo "❌ FAIL - Response: $DEVICE_RESP"

DEVICE_ID=$(echo "$DEVICE_RESP" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
DEVICE_DEVICE_ID=$(echo "$DEVICE_RESP" | grep -o '"device_id":"[^"]*"' | head -1 | cut -d'"' -f4)

# Test 4: List devices
echo -e "\n[TEST 4] List devices..."
LIST_DEVICES=$(curl -s -X GET "$API_URL/devices" \
  -H "Authorization: Bearer $TOKEN")
echo "$LIST_DEVICES" | grep -q "devices" && echo "✅ PASS" || echo "❌ FAIL"

# Test 5: Get device by ID
echo -e "\n[TEST 5] Get device by ID..."
GET_DEVICE=$(curl -s -X GET "$API_URL/devices/$DEVICE_ID" \
  -H "Authorization: Bearer $TOKEN")
echo "$GET_DEVICE" | grep -q "device_id" && echo "✅ PASS" || echo "❌ FAIL"

# Test 6: Upload model (requires file)
echo -e "\n[TEST 6] Upload model..."
# Create a dummy tflite file (just some bytes)
echo "dummy tflite model" > /tmp/dummy.tflite
UPLOAD_RESP=$(curl -s -X POST "$API_URL/models/upload" \
  -H "Authorization: Bearer $TOKEN" \
  -F "file=@/tmp/dummy.tflite" \
  -F "name=test-model" \
  -F "version=1" \
  -F "model_format=tflite")
echo "$UPLOAD_RESP" | grep -q "id" && echo "✅ PASS" || echo "❌ FAIL - Response: $UPLOAD_RESP"

MODEL_ID=$(echo "$UPLOAD_RESP" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

# Test 7: List models
echo -e "\n[TEST 7] List models..."
LIST_MODELS=$(curl -s -X GET "$API_URL/models" \
  -H "Authorization: Bearer $TOKEN")
echo "$LIST_MODELS" | grep -q "models" && echo "✅ PASS" || echo "❌ FAIL"

# Test 8: Create deployment
echo -e "\n[TEST 8] Create deployment..."
DEPLOY_RESP=$(curl -s -X POST "$API_URL/deployments" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d "{\"model_id\":\"$MODEL_ID\",\"target_type\":\"device\",\"target_id\":\"$DEVICE_ID\",\"rollout_strategy\":\"all_at_once\",\"rollout_percentage\":100}")
echo "$DEPLOY_RESP" | grep -q "id" && echo "✅ PASS" || echo "❌ FAIL - Response: $DEPLOY_RESP"

DEPLOYMENT_ID=$(echo "$DEPLOY_RESP" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)

# Test 9: List deployment devices
echo -e "\n[TEST 9] List deployment devices..."
DEPLOY_DEVICES=$(curl -s -X GET "$API_URL/deployments/$DEPLOYMENT_ID/devices" \
  -H "Authorization: Bearer $TOKEN")
echo "$DEPLOY_DEVICES" | grep -q "devices" && echo "✅ PASS" || echo "❌ FAIL"

# Test 10: WebSocket connection test (using wscat if available)
echo -e "\n[TEST 10] WebSocket endpoint..."
WS_RESP=$(curl -s -I -H "Authorization: Bearer $TOKEN" "$BASE_URL/ws" 2>&1)
echo "$WS_RESP" | grep -q "101" && echo "✅ PASS (WebSocket upgrade works)" || echo "❌ FAIL - WebSocket not responding"

echo -e "\n=== Test Summary ==="
echo "All API tests completed. Check output above for failures."
echo "Token: $TOKEN"
echo "Device ID: $DEVICE_ID"
echo "Model ID: $MODEL_ID"
echo "Deployment ID: $DEPLOYMENT_ID"
