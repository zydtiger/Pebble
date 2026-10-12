#!/usr/bin/env python3
"""Deterministic isolated app-server protocol fixture; no network or user files."""
import json
import os
import pathlib
import select
import sys
import time

root = pathlib.Path(__file__).resolve().parent
workspace = pathlib.Path.cwd()
options = json.loads((root / 'fake-options.json').read_text()) if (root / 'fake-options.json').exists() else {'mode': 'complete', 'artifact': True}
mode = options.get('mode', 'complete')
thread = 'synthetic-thread-' + str(os.getpid())
turn = None
waiting = False
input_buffer = ""
cancel_waiting = False
review_pending = False

def write(value):
    print(json.dumps(value), flush=True)

def event(method, params):
    write({'method': method, 'params': dict(threadId=thread, turnId=turn, **params)})

def finish(status='completed', text=None):
    global waiting
    text = text or options.get('text') or 'Verified synthetic result. Source: https://example.org/evidence'
    status = options.get('status', status) if status == 'completed' else status
    item = {'type': 'agentMessage', 'id': 'answer', 'phase': 'final_answer', 'text': text}
    outcome = {'id': turn, 'status': status, 'items': [item]}
    if options.get('runtime_error'):
        outcome['error'] = {'message': options['runtime_error']}
    if mode != 'restart':
        saved = root / ('fake-saved-staged-' + str(os.getpid()) + '.json')
        saved.write_text(json.dumps(outcome))
        saved.replace(root / ('fake-saved-' + thread + '.json'))
    if mode == 'url-loss':
        os._exit(0)  # The tool resolved, but its final echo arrives only through thread/read.
    if options.get('web_url'):
        event('item/completed', {'item': {'type': 'webSearch', 'id': 'visited', 'action': {'url': options['web_url']}}})
    # Stale callbacks must never overwrite the actual outcome or leak other rooms.
    write({'method': 'item/completed', 'params': {'threadId': 'wrong-thread', 'turnId': turn, 'item': dict(item, text='PRIVATE_OTHER_TASK')}})
    event('item/completed', {'item': item})
    event('turn/completed', {'turn': outcome})
    waiting = False

while True:
    if review_pending and (root / 'release-review').exists():
        if options['review_status'] == 'approved':
            event('guardianWarning', {'message': 'Automatic approval review approved (risk: low, authorization: high): Synthetic action is authorized.'})
        event('item/autoApprovalReview/completed', {'action': {'type': 'command', 'command': 'echo synthetic', 'cwd': str(workspace), 'source': 'shell'}, 'reviewId': 'review-1', 'targetItemId': 'reviewed-command', 'startedAtMs': 1, 'completedAtMs': 2, 'decisionSource': 'agent', 'review': {'status': options['review_status'], 'rationale': 'Synthetic review rationale: requested action exceeds authorization.'}})
        review_pending = False
        waiting = True
    if waiting and mode == 'auto-review' and (root / 'release').exists():
        finish()
    if waiting and mode in ('gated', 'url', 'url-loss', 'followup-gated', 'error-retry') and (root / 'release').exists():
        if mode == 'followup-gated':
            artifact = workspace / ('followup-' + turn + '.txt')
            artifact.write_text('current synthetic continuation artifact')
            event('item/completed', {'item': {'id': 'current-file', 'type': 'fileChange', 'status': 'completed', 'changes': [{'path': str(artifact)}]}})
        finish()
    if cancel_waiting and (root / 'confirm-cancel').exists():
        finish('interrupted', 'Cancelled fixture; earlier effects remain.')
        cancel_waiting = False
    if '\n' not in input_buffer:
        if not select.select([sys.stdin], [], [], 0.05)[0]:
            continue
        chunk = os.read(sys.stdin.fileno(), 65536)
        if not chunk:
            break
        input_buffer += chunk.decode()
        if '\n' not in input_buffer:
            continue
    line, input_buffer = input_buffer.split('\n', 1)
    value = json.loads(line)
    method = value.get('method')
    if method:
        with (root / 'fake-methods.jsonl').open('a') as log:
            log.write(json.dumps({'method': method, 'params': value.get('params', {})}) + '\n')
    def reply(result):
        write({'id': value['id'], 'result': result})
    if method == 'initialize':
        if mode == 'barrier-initialize':
            (root / 'at-barrier').write_text('initialize')
            while not (root / 'release').exists():
                time.sleep(0.01)
        reply({'userAgent': 'synthetic'})
    elif method == 'account/read':
        reply({'account': None if mode == 'unauthenticated' else {'type': 'chatgpt'}, 'requiresOpenaiAuth': True})
    elif method == 'model/list':
        reply({'data': [{'model': 'inherited-fixture-model'}], 'nextCursor': None})
    elif method == 'skills/list':
        reply({'data': [{'skills': [{'name': 'synthetic-calendar-routing', 'enabled': True}]}]})
    elif method == 'mcpServerStatus/list':
        reply({'data': [{'name': 'synthetic-mcp', 'authStatus': 'oAuth'}], 'nextCursor': None})
    elif method in ('thread/start', 'thread/resume'):
        if method == 'thread/resume':
            thread = value['params']['threadId']
        if mode == 'barrier-thread':
            (root / 'at-barrier').write_text(method)
            while not (root / 'release').exists():
                time.sleep(0.01)
        if mode == 'policy-rejected':
            write({'id': value['id'], 'error': {'code': -32602, 'message': 'Synthetic auto_review policy is unavailable under local requirements.'}})
        elif mode == 'unauthenticated':
            write({'id': value['id'], 'error': {'code': -32000, 'message': 'Login required'}})
        else:
            reply({'thread': {'id': thread}})
    elif method == 'thread/read':
        thread = value['params']['threadId']
        saved = json.loads((root / ('fake-saved-' + thread + '.json')).read_text()) if (root / ('fake-saved-' + thread + '.json')).exists() else {'id': 'turn-1', 'status': 'completed', 'items': [{'type': 'agentMessage', 'phase': 'final_answer', 'text': 'Recovered actual synthetic result'}]}
        reply({'thread': {'id': thread, 'turns': [saved]}})
    elif method == 'turn/start':
        if mode == 'turn-rejected':
            write({'id': value['id'], 'error': {'code': options.get('code', -32602), 'message': 'Synthetic invalid effort. Select a supported value.'}})
            continue
        count_path = root / ('fake-dispatch-count-' + thread)
        count = int(count_path.read_text()) + 1 if count_path.exists() else 1
        count_path.write_text(str(count))
        turn = 'turn-' + str(count)
        if mode in ('turn-start-loss', 'turn-start-loss-mismatch'):
            # The server accepted/persisted the turn, but its RPC response is lost.
            # UserMessage IDs are server-generated; clientId echoes clientUserMessageId.
            client_id = value['params']['clientUserMessageId']
            if mode == 'turn-start-loss-mismatch':
                client_id = 'another-submission'
            saved = {'id': turn, 'status': 'completed', 'items': [
                {'type': 'userMessage', 'id': 'server-generated-user-item', 'clientId': client_id, 'content': value['params']['input']},
                {'type': 'agentMessage', 'id': 'server-generated-answer', 'phase': 'final_answer', 'text': 'Recovered accepted turn. Source: https://example.org/accepted-turn'}
            ]}
            (root / ('fake-saved-' + thread + '.json')).write_text(json.dumps(saved))
            os._exit(0)
        if mode == 'restart':
            (root / ('fake-saved-' + thread + '.json')).write_text(json.dumps({'id': turn, 'status': 'completed', 'items': [{'type': 'agentMessage', 'phase': 'final_answer', 'text': 'Recovered actual synthetic result; completion notification was lost'}]}))
        reply({'turn': {'id': turn, 'status': 'inProgress'}})
        if options.get('commentary'):
            event('item/completed', {'item': {'type': 'agentMessage', 'phase': 'commentary', 'text': options['commentary']}})
            event('item/completed', {'item': {'type': 'agentMessage', 'phase': 'commentary', 'text': 'Later progress must not replace the task description.'}})
        if mode == 'disconnect':
            os._exit(0)
        if mode == 'auto-review':
            review = {'action': {'type': 'command', 'command': 'echo synthetic', 'cwd': str(workspace), 'source': 'shell'}, 'reviewId': 'review-1', 'targetItemId': 'reviewed-command', 'startedAtMs': 1, 'review': {'status': 'inProgress'}}
            stale = dict(review, review={'status': 'denied', 'rationale': 'STALE_REVIEW_MUST_NOT_DISPLAY'})
            write({'method': 'item/autoApprovalReview/completed', 'params': dict(threadId=thread, turnId='wrong-turn', **stale)})
            event('item/autoApprovalReview/started', review)
            review_pending = True
            continue
        if mode in ('error-fatal', 'error-delayed', 'error-retry'):
            error = {'message': 'Synthetic turn error; check the local configuration.', 'additionalDetails': 'PRIVATE_RAW_STACK_MUST_NOT_DISPLAY'}
            if mode == 'error-retry':
                write({'method': 'error', 'params': {'threadId': 'wrong-thread', 'turnId': turn, 'willRetry': False, 'error': error}})
                write({'method': 'error', 'params': {'threadId': thread, 'turnId': 'wrong-turn', 'willRetry': False, 'error': error}})
                write({'method': 'error', 'params': {'threadId': thread, 'willRetry': False, 'error': error}})
                event('error', {'error': error})  # Missing retry disposition is not a definitive failure.
                event('item/completed', {'item': {'type': 'commandExecution', 'id': 'ordinary-failed-tool', 'status': 'failed', 'exitCode': 1}})
                event('hook/started', {'status': 'inProgress'})
            event('error', {'willRetry': mode == 'error-retry', 'error': error})
            if mode == 'error-delayed':
                while not (root / 'release').exists():
                    time.sleep(0.01)
                time.sleep(0.35)  # Persistence/final frames can follow explicit failure later.
                finish('failed', 'Saved partial evidence after the explicit error. https://example.org/partial \ue200cite\ue202synthetic\ue201')
            elif mode == 'error-retry':
                waiting = True
            continue  # Fatal mode intentionally never sends turn/completed.
        event('item/started', {'item': {'id': 'web', 'type': 'webSearch'}})
        if mode.startswith('oversized-'):
            params = {'threadId': thread, 'turnId': turn, 'itemId': 'oversized-operation', 'startedAtMs': 1, 'reason': 'Review the complete synthetic scope.'}
            if mode == 'oversized-permissions':
                params.update(cwd=str(workspace), permissions={'fileSystem': {'write': ['/synthetic/' + 'x' * 100 + str(i) for i in range(100)] + ['/synthetic/last-authorized-root']}})
                native_method = 'item/permissions/requestApproval'
            elif mode == 'oversized-file':
                event('item/started', {'item': {'id': params['itemId'], 'type': 'fileChange', 'status': 'inProgress', 'changes': [{'path': 'synthetic.txt', 'kind': {'type': 'update'}, 'diff': '+' + 'x' * 18000 + '\n+last actual change'}]}})
                native_method = 'item/fileChange/requestApproval'
            else:
                params.update(command='echo synthetic ' + 'x' * 9000 if mode == 'oversized-command' else 'write /synthetic/actual-scope.txt', cwd=str(workspace), commandActions=None)
                if mode == 'oversized-reason':
                    params['reason'] = 'x' * 9000
                if mode == 'oversized-network':
                    params.update(command=None, commandActions=None, networkApprovalContext={'host': 'x' * 9000 + '.example.com', 'protocol': 'https'})
                native_method = 'item/commandExecution/requestApproval'
            write({'id': 'interaction-1', 'method': native_method, 'params': params})
        elif mode in ('url', 'url-loss', 'url-invalid'):
            write({'id': 'interaction-1', 'method': 'mcpServer/elicitation/request', 'params': {'threadId': thread, 'turnId': turn, 'serverName': 'synthetic', 'mode': 'url', 'elicitationId': 'synthetic-auth', 'message': 'Complete the synthetic authorization, then confirm.', 'url': options.get('url', 'https://example.org/authorize?state=secret' if mode in ('url', 'url-loss') else 'file:///synthetic/authorize?state=secret')}})
        elif mode == 'file-overlap':
            for item in ('file-A', 'file-B'):
                event('item/started', {'item': {'id': item, 'type': 'fileChange', 'changes': [{'path': item + '.txt', 'diff': 'proposed ' + item}]}})
            for index, item in enumerate(('file-A', 'file-B'), 1):
                write({'id': 'interaction-' + str(index), 'method': 'item/fileChange/requestApproval', 'params': {'threadId': thread, 'turnId': turn, 'itemId': item, 'startedAtMs': 1}})
        elif mode in ('form-mixed', 'form-optional', 'form-choices', 'form-unsupported', 'form-failed', 'form-constrained', 'form-pattern'):
            schema = {'type': 'object', 'properties': {'name': {'type': 'string'}, 'count': {'type': 'integer'}}, 'additionalProperties': False}
            if mode == 'form-mixed':
                schema['required'] = ['name']
            if mode == 'form-choices':
                schema = {'type': 'object', 'properties': {
                    'format': {'type': 'string', 'oneOf': [{'const': 'md', 'title': 'Markdown document'}, {'const': 'txt', 'title': 'Plain text'}]},
                    'untitled': {'type': 'array', 'items': {'type': 'string', 'enum': ['red', 'blue']}, 'minItems': 1},
                    'titled': {'type': 'array', 'items': {'anyOf': [{'const': 'src', 'title': 'Source files'}, {'const': 'docs', 'title': 'Documentation'}]}},
                    'optional': {'type': 'array', 'items': {'type': 'string', 'enum': ['extra']}}
                }, 'required': ['format', 'untitled', 'titled'], 'additionalProperties': False}
            if mode == 'form-constrained':
                schema = {'type': 'object', 'properties': {
                    'age': {'type': 'integer', 'description': 'Eligible age for this synthetic request.', 'minimum': 18, 'maximum': 21, 'default': 18},
                    'amount': {'type': 'number', 'description': 'Requested fraction.', 'minimum': 0.5, 'maximum': 1.5},
                    'code': {'type': 'string', 'description': 'Optional short public label.', 'minLength': 2, 'maxLength': 4, 'default': 'AB'},
                    'email': {'type': 'string', 'description': 'Optional contact.', 'format': 'email'},
                    'secret': {'type': 'string', 'writeOnly': True, 'description': 'Enter this value locally.', 'minLength': 4, 'maxLength': 8, 'default': 'SECRET_DEFAULT_MUST_NOT_DISPLAY'},
                    'areas': {'type': 'array', 'description': 'Choose project areas.', 'items': {'type': 'string', 'enum': ['src', 'docs']}, 'minItems': 1, 'maxItems': 2},
                    'flag': {'type': 'boolean', 'description': 'Optional switch.', 'default': False}
                }, 'required': ['age', 'areas'], 'additionalProperties': False}
            if mode == 'form-pattern':
                schema = {'type': 'object', 'properties': {'name': {'type': 'string', 'pattern': '[A-Z]+'}}}
            if mode in ('form-unsupported', 'form-failed'):
                schema = {'type': 'object', 'properties': {'nested': {'type': 'object', 'properties': {'value': {'type': 'string'}}}}}
            form = {'id': 'interaction-1', 'method': 'mcpServer/elicitation/request', 'params': {'threadId': thread, 'turnId': turn, 'serverName': 'synthetic', 'mode': 'openai/form' if mode in ('form-constrained', 'form-pattern') else 'form', 'message': 'Synthetic form', 'requestedSchema': schema}}
            if mode == 'form-pattern':
                for key in ('threadId', 'turnId'):
                    write(dict(form, params=dict(form['params'], **{key: 'stale-unrelated'})))
            write(form)
            if mode == 'form-pattern':
                write(form)
        elif mode in ('question', 'approval', 'overlap', 'approval-actions', 'approval-stdin', 'approval-network', 'cancel-approval', 'cancel-question'):
            params = {'threadId': thread, 'turnId': turn, 'itemId': 'interaction'}
            if mode in ('question', 'overlap', 'cancel-question'):
                params.update(isBlocking=True, questions=[{'id': 'choice', 'header': 'Scope and cost', 'question': 'Which synthetic artifact?', 'isSecret': options.get('secret_question', False), 'options': [{'label': 'Small', 'description': 'Read one synthetic file; no modifications.'}, {'label': 'Large', 'description': 'Inspect all synthetic files; higher latency.'}]}])
                native_method = 'item/tool/requestUserInput'
            else:
                params.update(command='write synthetic output.txt', cwd=str(workspace), reason='Synthetic fixture requests one scoped write.', startedAtMs=1)
                if mode == 'approval-actions':
                    params.update(command=None, commandActions=[{'type': 'read', 'command': 'cat README.md', 'name': 'README.md', 'path': 'README.md'}, {'type': 'search', 'command': 'rg TODO src', 'path': 'src', 'query': 'TODO'}, {'type': 'listFiles', 'command': 'ls docs', 'path': 'docs'}])
                if mode == 'approval-network':
                    params.update(command=None, commandActions=None, reason='Codex requests permission to proceed.', networkApprovalContext={'host': 'api.example.com', 'protocol': 'https'})
                if mode == 'approval-stdin':
                    params.update(command=None, kind='writeStdin', commandActions=None)
                native_method = 'item/commandExecution/requestApproval'
            write({'id': 'interaction-1', 'method': native_method, 'params': params})
            if mode in ('cancel-approval', 'cancel-question'):
                write({'id': 'interaction-2', 'method': native_method, 'params': params})
            if mode == 'overlap':
                write({'id': 'interaction-2', 'method': native_method, 'params': params})
                write({'id': 'interaction-2', 'method': native_method, 'params': params})
                write({'method': 'serverRequest/resolved', 'params': {'threadId': thread, 'requestId': 'stale-unrelated'}})
        elif mode in ('gated', 'cancel', 'restart', 'cancel-ack-only', 'followup-gated'):
            waiting = True
        else:
            if options.get('artifact'):
                artifact = workspace / 'output.txt'
                artifact.write_text('synthetic artifact')
                event('item/completed', {'item': {'id': 'file', 'type': 'fileChange', 'status': 'completed', 'changes': [{'path': str(artifact)}]}})
            finish()
    elif method == 'turn/interrupt':
        reply({})
        if mode == 'form-failed':
            continue  # A confirmed failed turn cannot be replaced by a later interrupt.
        if mode in ('cancel-approval', 'cancel-question'):
            cancel_waiting = True
            write({'id': 'interaction-3', 'method': native_method, 'params': params})
            write({'method': 'serverRequest/resolved', 'params': {'threadId': thread, 'requestId': 'interaction-1'}})
            (root / 'late-request-sent').write_text('synthetic')
        elif mode == 'cancel-ack-only' or options.get('interrupt_unconfirmed'):
            saved = {'id': turn, 'status': 'inProgress', 'items': [{'id': 'partial', 'type': 'agentMessage', 'phase': 'final_answer', 'text': 'Saved partial effects; outcome not confirmed.'}]}
            (root / ('fake-saved-' + thread + '.json')).write_text(json.dumps(saved))
        else:
            finish('interrupted', options.get('text') or 'Interrupted fixture; earlier effects remain.')
    elif not method and value.get('id') in ('interaction-1', 'interaction-2', 'interaction-3'):
        with (root / 'fake-responses.jsonl').open('a') as log:
            log.write(json.dumps(value) + '\n')
        if mode in ('cancel-approval', 'cancel-question'):
            continue
        if mode in ('overlap', 'file-overlap') and value['id'] == 'interaction-1':
            write({'method': 'serverRequest/resolved', 'params': {'threadId': thread, 'requestId': 'interaction-1'}})
            continue
        (root / 'fake-response.json').write_text(json.dumps(value))
        if mode in ('url', 'url-loss'):
            write({'method': 'serverRequest/resolved', 'params': {'threadId': thread, 'requestId': 'interaction-1'}})
            waiting = True  # Keep the resolved state observable before the terminal callback.
        elif 'error' in value:
            if mode == 'form-failed':
                runtime_error = {'message': 'Synthetic connected tool is unavailable; repair its local configuration. ' + 'x' * 5000, 'additionalDetails': 'PRIVATE_RAW_STACK_MUST_NOT_DISPLAY'}
                saved = {'id': turn, 'status': 'failed', 'items': [], 'error': runtime_error}
                (root / ('fake-saved-' + thread + '.json')).write_text(json.dumps(saved))
                event('turn/completed', {'turn': saved})
            else:
                waiting = True
        elif value.get('result', {}).get('decision') == 'decline' or value.get('result', {}).get('action') == 'decline' or value.get('result', {}).get('permissions') == {}:
            waiting = True
        else:
            finish(text='Interaction completed with actual user response: ' + json.dumps(value['result']))
