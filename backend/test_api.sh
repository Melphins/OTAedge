#!/bin/bash
set -e

BASE_URL="http://127.0.0.1:3000"

echo "=== OTAedge Backend Test Suite ==="
echo ""

# Test 1: Health check
echo "Test 1: Server is running..."
curl -s $BASE_URL/api/models > /dev/null && echo "✓ Server responding" || echo "✗ Server not responding"
echo ""

# Test 2: Register a new user
echo "Test 2: User registration..."
REGISTER_RESPONSE=$(curl -s -X POST $BASE_URL/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"test@otaedge.com","username":"testuser","password":"testpass123"}')
echo "$REGISTER_RESPONSE" | jq -r '.token' > /tmp/token.txt 2>/dev/null
if [ $? -eq 0 ]; then
    echo "✓ Registration successful"
else
    echo "✗ Registration failed"
fi
echo ""

# Test 3: Login
echo "Test 3: User login..."
LOGIN_RESPONSE=$(curl -s -X POST $BASE_URL/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"test@otaedge.com","password":"testpass123"}')
echo "$LOGIN_RESPONSE" | jq -r '.token' > /tmp/token2.txt 2>/dev/null
if [ $? -eq 0 ]; then
    echo "✓ Login successful"
    TOKEN=$(cat /tmp/token2.txt)
else
    echo "✗ Login failed"
fi
echo ""

# Test 4: Create device with auth
echo "Test 4: Create device (with authentication)..."
DEVICE_RESPONSE=$(curl -s -X POST $BASE_URL/api/devices \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"name":"Test Device","device_type":"raspberry_pi"}')
echo "$DEVICE_RESPONSE" | jq '.device_id' > /dev/null 2>&1
if [ $? -eq 0 ]; then
    echo "✓ Device created"
    DEVICE_ID=$(echo "$DEVICE_RESPONSE" | jq -r '.id')
    DEVICE_TOKEN=$(echo "$DEVICE_RESPONSE" | jq -r '.token')
    echo "  Device ID: $DEVICE_ID"
    echo "  Device token: $DEVICE_TOKEN"
else
    echo "✗ Device creation failed"
    echo "  Response: $DEVICE_RESPONSE"
fi
echo ""

# Test 5: List devices
echo "Test 5: List all devices (with auth)..."
DEVICES=$(curl -s -H "Authorization: Bearer $TOKEN" $BASE_URL/api/devices)
echo "$DEVICES" | jq '.[] | .id' > /dev/null 2>&1
if [ $? -eq 0 ]; then
    COUNT=$(echo "$DEVICES" | jq 'length')
    echo "✓ Devices listed (count: $COUNT)"
else
    echo "✗ Failed to list devices"
    echo "  Response: $DEVICES"
fi
echo ""

# Test 6: Get device by ID
echo "Test 6: Get device by ID (with auth)..."
if [ -n "$DEVICE_ID" ]; then
    SINGLE_DEVICE=$(curl -s -H "Authorization: Bearer $TOKEN" $BASE_URL/api/devices/$DEVICE_ID)
    echo "$SINGLE_DEVICE" | jq '.id' > /dev/null 2>&1
    if [ $? -eq 0 ]; then
        echo "✓ Device retrieved successfully"
    else
        echo "✗ Failed to get device"
        echo "  Response: $SINGLE_DEVICE"
    fi
else
    echo "⚠ Skipping (no device ID from previous test)"
fi
echo ""

# Test 7: Create device without auth (should fail)
echo "Test 7: Create device without authentication (should fail)..."
HTTP_CODE=$(curl -s -o /tmp/no_auth_response -w "%{http_code}" -X POST $BASE_URL/api/devices \
  -H "Content-Type: application/json" \
  -d '{"name":"No Auth Device"}')
if [ "$HTTP_CODE" = "401" ]; then
    echo "✓ Unauthorized as expected (HTTP 401)"
else
    echo "⚠ Unexpected HTTP status: $HTTP_CODE (expected 401)"
    echo "  Response: $(cat /tmp/no_auth_response)"
fi
echo ""

# Test 8: List models
echo "Test 8: List models (with auth)..."
HTTP_CODE=$(curl -s -o /tmp/models_response -w "%{http_code}" -H "Authorization: Bearer $TOKEN" $BASE_URL/api/models)
if [ "$HTTP_CODE" = "200" ]; then
    cat /tmp/models_response | jq '.' > /dev/null 2>&1
    if [ $? -eq 0 ]; then
        COUNT=$(cat /tmp/models_response | jq 'length')
        echo "✓ Models endpoint working (count: $COUNT)"
    else
        echo "⚠ Models endpoint returned 200 but invalid JSON"
    fi
else
    echo "✗ Models endpoint failed with HTTP $HTTP_CODE"
    echo "  Response: $(cat /tmp/models_response)"
fi
echo ""

# Test 9: WebSocket endpoint check
echo "Test 9: WebSocket endpoint (HTTP upgrade check)..."
WS_RESPONSE=$(curl -s -I -N -H "Connection: Upgrade" -H "Upgrade: websocket" \
  -H "Host: 127.0.0.1:3000" -H "Origin: http://127.0.0.1:3000" \
  $BASE_URL/ws 2>&1 | head -1)
if echo "$WS_RESPONSE" | grep -q "101"; then
    echo "✓ WebSocket endpoint responding"
else
    echo "⚠ WebSocket upgrade response: $WS_RESPONSE"
fi
echo ""

# Summary
echo "=== Test Summary ==="
echo "All basic API tests completed!"
echo "Token: $TOKEN (use for authenticated requests)"
echo "Device ID: $DEVICE_ID"
echo "Device Token: $DEVICE_TOKEN"
echo ""
echo "Next steps:"
echo "1. Test WebSocket connection with a client"
echo "2. Add a model to database and verify /api/models"
echo "3. Test device heartbeat via WebSocket"
echo "4. Implement auth middleware for protected routes"
