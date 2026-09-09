"""Loopback-only UI verification provider. Replies are explicitly mock text.

Run manually with Python, then configure http://127.0.0.1:18443/v1 and model
mock-course-provider in an isolated test library. Never use as a real model.
"""
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class MockProvider(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != '/v1/chat/completions':
            self.send_error(404)
            return
        length = int(self.headers.get('Content-Length', '0'))
        if not 0 < length <= 1024 * 1024:
            self.send_error(413)
            return
        request = json.loads(self.rfile.read(length))
        source = json.loads(request['messages'][-1]['content'])
        identifiers = [segment['segmentId'] for segment in source['segments']]
        content = {'content': '模拟接口验证：这条笔记仅用于检查选择范围和引用跳转。'
                              f'[引用:{identifiers[0]}]', 'citations': identifiers[:1]}
        body = json.dumps({'choices': [{'message': {'content': json.dumps(content, ensure_ascii=False)}}]}, ensure_ascii=False).encode('utf-8')
        self.send_response(200)
        self.send_header('Content-Type', 'application/json; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        print(json.dumps({'test': 'mock', 'selectedSegments': len(identifiers)}), flush=True)


if __name__ == '__main__':
    print('Mock provider listening only on http://127.0.0.1:18443/v1', flush=True)
    HTTPServer(('127.0.0.1', 18443), MockProvider).serve_forever()
