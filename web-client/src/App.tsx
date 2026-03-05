import { useState, useRef } from 'react';
import { Mic, MicOff, Settings, ShieldCheck, Activity, Brain } from 'lucide-react';
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
    console.log('Toggle clicked, current state:', isConnected);
    if (isConnected) {
      disconnect();
    } else {
      await connect();
      console.log('Neural link connection sequence initiated to:', FACTORY_URL);
    }
  };

  const connect = async () => {
    console.log('Attempting to connect to:', FACTORY_URL, 'with agent:', agentId);
    setTranscript('Connecting to factory...');
    try {
      const ws = new WebSocket(`${FACTORY_URL}?agent_id=${agentId}`);
      wsRef.current = ws;

      const stream = new AudioStream((base64) => {
        if (ws.readyState === WebSocket.OPEN) {
          if (Math.random() < 0.05) console.log('Sending audio chunk...', base64.substring(0, 30));
          ws.send(JSON.stringify({ type: 'audio', data: base64 }));
        }
      });
      audioStreamRef.current = stream;

      ws.onopen = async () => {
        console.log('WebSocket opened');
        try {
          await stream.start();
          setIsConnected(true);
          setIsRecording(true);
          setTranscript('Neural link established. Speak now.');
        } catch (micErr) {
          console.error('Microphone error:', micErr);
          setTranscript('Microphone access denied or failed.');
          ws.close();
        }
      };

      ws.onmessage = async (event) => {
        try {
          const msg = JSON.parse(event.data);
          if (msg.type === 'audio') {
            await stream.playChunk(msg.data);
          } else if (msg.type === 'protocol') {
            console.log('Protocol Message:', msg.data);
          }
        } catch (e) {
          // If not JSON, it might be raw base64 or a status string
          console.debug('Received non-JSON message or parse error:', e);
          // Fallback: try to play as base64 audio if it looks like one
          if (typeof event.data === 'string' && event.data.length > 100) {
            await stream.playChunk(event.data);
          }
        }
      };

      ws.onclose = (e) => {
        console.log('WebSocket closed:', e.code, e.reason);
        disconnect();
      };
    } catch (err) {
      console.error('Connection failed:', err);
      setTranscript('Connection failed. Retrying...');
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
        <div className="status-item">
          <Activity size={14} className={isConnected ? 'text-accent' : ''} />
          <span>Stream: {isConnected ? 'Active' : 'Idle'}</span>
        </div>
        <div className="status-item">
          <Brain size={14} />
          <span>Core: Vertex AI L4</span>
        </div>
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
        <h1 className="agent-name">Jenny</h1>
        <p className="agent-role">Neuroscience & Growth Consultant</p>
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
