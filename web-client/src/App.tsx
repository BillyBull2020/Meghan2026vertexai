import { useState, useRef } from 'react';
import { Mic, MicOff, Settings, ShieldCheck } from 'lucide-react';
import { AudioStream } from './lib/AudioStream';
import './App.css';

const FACTORY_URL = window.location.hostname === 'localhost'
  ? 'ws://localhost:3000/web-session'
  : 'wss://ironclaw-factory-uc4oqbsooa-uc.a.run.app/web-session';

function App() {
  const [isConnected, setIsConnected] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [transcript, setTranscript] = useState('Click the orb to start the neural link...');
  const [agentId, setAgentId] = useState('template_neuro_sales_01');

  const wsRef = useRef<WebSocket | null>(null);
  const audioStreamRef = useRef<AudioStream | null>(null);

  const toggleConnection = async () => {
    if (isConnected) {
      disconnect();
    } else {
      await connect();
    }
  };

  const connect = async () => {
    console.log('--- NEURAL LINK SEQUENCE START ---');
    setTranscript('Connecting to factory...');

    // CRITICAL: Initialize AudioStream IMMEDIATELY in the click event loop.
    // This ensures context is not suspended by Chromium.
    const stream = new AudioStream((base64) => {
      if (wsRef.current?.readyState === WebSocket.OPEN) {
        if (Math.random() < 0.01) console.log('Outgoing audio clip...');
        wsRef.current.send(JSON.stringify({ type: 'audio', data: base64 }));
      }
    });
    audioStreamRef.current = stream;

    try {
      // 1. Start audio first (user gesture turn)
      await stream.start();
      console.log('Audio system warmed up.');

      // 2. Open WebSocket
      console.log('Opening WebSocket to:', FACTORY_URL);
      const ws = new WebSocket(`${FACTORY_URL}?agent_id=${agentId}`);
      wsRef.current = ws;

      ws.onopen = () => {
        console.log('WebSocket Opened successfully');
        setIsConnected(true);
        setIsRecording(true);
        setTranscript('Neural link established. Speak now.');
      };

      ws.onmessage = async (event) => {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === 'audio') {
            await stream.playChunk(msg.data);
          } else if (msg.type === 'text') {
            setTranscript(msg.data); // Update transcript with new text
          } else {
            console.log('Server message:', msg);
          }
        } catch (e) {
          // Backward compatibility for raw base64
          if (typeof event.data === 'string' && event.data.length > 50) {
            await stream.playChunk(event.data);
          }
        }
      };

      ws.onclose = (e) => {
        console.log('WebSocket Closed:', e.code, e.reason);
        disconnect();
      };

      ws.onerror = (err) => {
        console.error('WebSocket Error:', err);
        setTranscript('Connection error. Check console.');
      };

    } catch (err) {
      console.error('Handshake failed:', err);
      setTranscript('Neural link failed. Resetting...');
      disconnect();
    }
  };

  const disconnect = () => {
    wsRef.current?.close();
    audioStreamRef.current?.stop();
    setIsConnected(false);
    setIsRecording(false);
    setTranscript('Neural link offline.');
  };

  return (
    <div className="ambient-container">
      <div className="status-bar">
        <p className={`status ${isConnected ? 'active' : ''}`}>
          {isConnected ? 'Stream: Active' : 'Stream: Offline'}
        </p>


        <p className="hint">
          {isConnected
            ? 'Neural link established. Speak now.'
            : 'Click the orb to initiate neural uplink.'}
        </p>
        <div className="status-item">
          <ShieldCheck size={14} />
          <span>Auth: Secured</span>
        </div>
      </div>

      <div className="voice-orb-container" onClick={toggleConnection}>
        <div className={`pulse-ring ${isRecording ? 'active' : ''}`}></div>
        <div className={`pulse-ring pulse-ring-delayed ${isRecording ? 'active' : ''}`}></div>
        <div className="voice-orb">
          {isRecording ? <Mic size={40} color="black" /> : <MicOff size={40} color="black" />}
        </div>
      </div>

      <div className="agent-info">
        <h1 className="agent-name">
          {agentId.includes('jenny') || agentId.includes('template') ? 'Jenny' :
            agentId.includes('mark') ? 'Mark' :
              agentId.includes('aria') ? 'Aria' : 'Jules'}
        </h1>
        <p className="agent-role">
          {agentId.includes('jenny') || agentId.includes('template') ? 'Neuroscience & Growth Consultant' :
            agentId.includes('mark') ? 'ROI & Closing Specialist' :
              agentId.includes('aria') ? 'Inbound Receptionist' : 'Systems Architect'}
        </p>
      </div>

      <div className="transcript-container">
        <p className="transcript-text">{transcript}</p>
      </div>

      <div className="controls">
        <select
          className="control-btn"
          value={agentId}
          onChange={(e) => setAgentId(e.target.value)}
          disabled={isConnected}
          aria-label="Select Agent"
          title="Select Agent Profile"
        >
          <option value="template_neuro_sales_01">Jenny (Sales)</option>
          <option value="mark_roi_closer_01">Mark (Closer)</option>
          <option value="aria_receptionist_01">Aria (Reception)</option>
          <option value="jules_architect_01">Jules (Workshops)</option>
        </select>
        <button className="control-btn" onClick={() => window.location.reload()} aria-label="Settings" title="Reset Session">
          <Settings size={16} />
        </button>
      </div>
    </div>
  );
}

export default App;
