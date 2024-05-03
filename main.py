import json
import threading
import uuid
from time import sleep

import requests
import websocket
from websocket import WebSocket


def test():
    task_id = uuid.uuid4()
    task_group = "2322fafb-ba0c-4dcf-932a-d7392817e723"

    response = requests.post("http://127.0.0.1:8080/v1/bp/u/", files={
        ('original_image', ('original.jpg', open('img.jpg', 'rb').read())),
        ('task_group', str(task_group)),
        ('country', str('NP'))
    })

    if response.status_code != 200:
        print("Connection failed", response.status_code)
        print('data', response.text)
    else:
        print("file uploaded")
        result = json.loads(response.text)
        # print(result)
        task_id = result['data']['key']

    def opeedn(websocket: WebSocket):
        print("opened")
        websocket.send(json.dumps({
            "key": str(task_id)
        }))

    def message(ws, data):
        data = json.loads(data)
        if data.get('status') == 'success':
            print('Completed')
        elif data.get('status') == 'failed':
            print('failed')

    def success(websocket):
        print("Connected")

    def closed(websocket, data):
        print("Closed")

    def error(websocket, code):
        print("error: {}", code)

    ws = websocket.WebSocketApp(f"ws://127.0.0.1:8080/ws/remove-background/{task_group}/",
                                on_open=opeedn,
                                on_message=message,
                                on_error=error,
                                on_close=closed)
    ws.run_forever(dispatcher=None,
                   reconnect=5)


for i in range(1):
    thread = threading.Thread(target=test)
    thread.start()
    sleep(0.01)
    print(i)
