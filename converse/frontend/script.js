const API_BASE_URL = window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1'
    ? 'http://localhost:3000'
    : `http://${window.location.hostname}:3000`;

const TTS_API_URL = 'https://api.on-demand.io/services/v1/public/service/execute/text_to_speech';
const POLL_INTERVAL = 1000; // 1 second

console.log('API Base URL:', API_BASE_URL);

const messageForm = document.getElementById('messageForm');
const messageInput = document.getElementById('messageInput');
const sendBtn = document.getElementById('sendBtn');
const chatBox = document.getElementById('chatBox');
const userIdInput = document.getElementById('userIdInput');
const statusIndicator = document.getElementById('statusIndicator');

let pollTimeout;
let currentUserId;

// Clear welcome message on first message
let firstMessage = true;

messageForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    
    const message = messageInput.value.trim();
    if (!message) return;

    currentUserId = userIdInput.value.trim() || 'user123';
    
    // Clear welcome message on first message
    if (firstMessage) {
        chatBox.innerHTML = '';
        firstMessage = false;
    }

    // Display user message
    addMessage(message, 'user');
    messageInput.value = '';
    messageInput.focus();

    // Disable send button and show processing
    sendBtn.disabled = true;
    updateStatus('Processing...', 'processing');

    try {
        // Send message to backend
        const response = await fetch(`${API_BASE_URL}/chat`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({
                userId: currentUserId,
                message: message
            })
        });

        if (!response.ok) {
            throw new Error(`HTTP error! status: ${response.status}`);
        }

        const data = await response.json();
        console.log('✅ Backend response received:', data);

        // Start polling for results
        startPolling(currentUserId);

    } catch (err) {
        console.error('Full error:', err);
        addMessage(`Error: ${err.message}. Make sure the backend is running on http://localhost:3000`, 'error');
        sendBtn.disabled = false;
        updateStatus('Error', 'error');
    }
});

function addMessage(text, type) {
    const messageEl = document.createElement('div');
    messageEl.className = `message ${type}`;

    const contentEl = document.createElement('div');
    contentEl.className = 'message-content';

    // For bot messages with object data, format nicely
    if (type === 'bot' && typeof text === 'object') {
        contentEl.innerHTML = formatBotResponse(text);
    } else if (type === 'bot' && typeof text === 'string') {
        contentEl.innerHTML = escapeHtml(text);
    } else {
        contentEl.textContent = text;
    }

    messageEl.appendChild(contentEl);

    // Add TTS button for bot messages
    if (type === 'bot') {
        const ttsBtn = document.createElement('button');
        ttsBtn.className = 'tts-btn';
        ttsBtn.title = 'Listen';
        ttsBtn.innerHTML = '🔊';
        ttsBtn.onclick = () => playTextToSpeech(extractPlainText(contentEl.innerText));
        messageEl.appendChild(ttsBtn);
    }

    const metaEl = document.createElement('div');
    metaEl.className = 'message-meta';
    metaEl.textContent = new Date().toLocaleTimeString();
    messageEl.appendChild(metaEl);

    chatBox.appendChild(messageEl);
    chatBox.scrollTop = chatBox.scrollHeight;
}

function extractPlainText(html) {
    const div = document.createElement('div');
    div.innerHTML = html;
    return div.innerText || div.textContent || '';
}

function formatBotResponse(responseObj) {
    let html = '';

    // Show only the message content - extract it smartly
    let messageContent = '';

    if (responseObj.chatResponse) {
        const chat = responseObj.chatResponse;
        
        if (typeof chat === 'string') {
            messageContent = chat;
        } else if (chat.data?.answer) {
            // Extract answer from the nested data structure (most common format)
            messageContent = chat.data.answer;
        } else if (chat.content) {
            messageContent = chat.content;
        } else if (chat.data?.content) {
            messageContent = chat.data.content;
        } else if (chat.data?.message) {
            messageContent = chat.data.message;
        } else if (chat.message) {
            messageContent = chat.message;
        } else {
            // As a last resort, stringify chat
            try {
                messageContent = JSON.stringify(chat, null, 2);
            } catch (e) {
                messageContent = String(chat);
            }
        }
    }

    // Merge workflow result into the displayed message without breaking formatting
    if (responseObj.workflowResult) {
        const wf = responseObj.workflowResult;
        let wfText = '';

        if (!wf) wfText = '';
        else if (typeof wf === 'string') wfText = wf;
        else if (wf.output) wfText = (typeof wf.output === 'string') ? wf.output : JSON.stringify(wf.output, null, 2);
        else if (wf.data) wfText = (typeof wf.data === 'string') ? wf.data : JSON.stringify(wf.data, null, 2);
        else if (wf.result) wfText = (typeof wf.result === 'string') ? wf.result : JSON.stringify(wf.result, null, 2);
        else if (wf.message) wfText = wf.message;
        else if (wf.executionID) wfText = `Execution ID: ${wf.executionID}`;
        else {
            try { wfText = JSON.stringify(wf, null, 2); } catch (e) { wfText = String(wf); }
        }

        if (wfText) {
            if (messageContent) messageContent += '\n\n' + 'Workflow result:\n' + wfText;
            else messageContent = 'Workflow result:\n' + wfText;
        }
    }

    // If we extracted content, show it in a simple box
    if (messageContent) {
        html = `<div style="padding: 12px; background: #f0f0f0; border-radius: 8px; line-height: 1.6; white-space: pre-wrap;">${escapeHtml(messageContent)}</div>`;
    } else {
        // Fallback: show raw JSON if no message found
        html = `<div style="padding: 12px; background: #f0f0f0; border-radius: 8px; font-size: 12px; white-space: pre-wrap; word-break: break-word;">${escapeHtml(JSON.stringify(responseObj, null, 2))}</div>`;
    }

    return html;
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function startPolling(userId) {
    const poll = async () => {
        try {
            console.log('Polling for results, userId:', userId);
            const response = await fetch(`${API_BASE_URL}/results?userId=${encodeURIComponent(userId)}`);
            
            if (!response.ok) {
                console.error('Poll response not ok:', response.status);
                // Retry on error
                pollTimeout = setTimeout(poll, POLL_INTERVAL);
                return;
            }

            const data = await response.json();
            console.log('Poll result:', data);

            if (data.status === 'done') {
                // Results ready
                console.log('Results ready, displaying...');
                addMessage(data, 'bot');
                sendBtn.disabled = false;
                updateStatus('Ready', 'ready');
                clearTimeout(pollTimeout);

            } else if (data.status === 'processing') {
                // Still processing, poll again
                console.log('Still processing, polling again...');
                pollTimeout = setTimeout(poll, POLL_INTERVAL);

            } else if (data.status === 'error') {
                // Error occurred
                console.error('Error from backend:', data.error);
                addMessage(`Error: ${data.error}`, 'error');
                sendBtn.disabled = false;
                updateStatus('Error', 'error');
                clearTimeout(pollTimeout);

            } else if (data.status === 'not_found') {
                // Request not yet created, poll again
                console.log('Request not found yet, polling again...');
                pollTimeout = setTimeout(poll, POLL_INTERVAL);
            }

        } catch (err) {
            console.error('Polling error:', err);
            // Continue polling on network errors
            pollTimeout = setTimeout(poll, POLL_INTERVAL);
        }
    };

    poll();
}

function updateStatus(text, statusClass) {
    statusIndicator.textContent = text;
    statusIndicator.className = 'status-indicator';
    if (statusClass && statusClass !== 'ready') {
        statusIndicator.classList.add(statusClass);
    }
}

// Set focus on input when page loads
window.addEventListener('load', () => {
    messageInput.focus();
});

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    if (pollTimeout) {
        clearTimeout(pollTimeout);
    }
});

// Text-to-Speech function
async function playTextToSpeech(text) {
    if (!text.trim()) {
        alert('No text to convert');
        return;
    }

    try {
        console.log('Converting to speech:', text);
        
        const response = await fetch(`${API_BASE_URL}/tts`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({
                text: text.substring(0, 4096)
            })
        });

        if (!response.ok) {
            console.error('TTS Error:', response.status);
            useBrowserSpeech(text);
            return;
        }

        const data = await response.json();
        console.log('TTS Response:', data);

        // If API returns useBrowserTTS flag, use browser speech
        if (data.useBrowserTTS) {
            console.log('Falling back to browser TTS');
            useBrowserSpeech(text);
            return;
        }

        // If API has audio URL, play it
        if (data.data?.audioUrl) {
            const audio = new Audio(data.data.audioUrl);
            audio.play();
            console.log('Playing audio...');
        }
    } catch (err) {
        console.error('TTS Error:', err);
        useBrowserSpeech(text);
    }
}

// Fallback: Use browser's Web Speech API
function useBrowserSpeech(text) {
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.rate = 1.0;
    utterance.pitch = 1.0;
    utterance.volume = 1.0;
    
    window.speechSynthesis.cancel(); // Cancel any ongoing speech
    window.speechSynthesis.speak(utterance);
    console.log('Using browser Web Speech API');
}
