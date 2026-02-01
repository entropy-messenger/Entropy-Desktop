
import { writable, get } from 'svelte/store';
import { network } from './network';
import { signalManager } from './signal_manager';
import { userStore } from './user_store';
import { addMessage } from './store';
import type { Message } from './types';

export interface CallState {
    isActive: boolean;
    peerHash: string | null;
    callId: string | null;
    type: 'voice' | 'video' | null;
    localStream: MediaStream | null;
    remoteStream: MediaStream | null;
    isIncoming: boolean;
    status: 'idle' | 'calling' | 'ringing' | 'connected' | 'ended';
    duration: number; 
}

const initialCallState: CallState = {
    isActive: false,
    peerHash: null,
    callId: null,
    type: null,
    localStream: null,
    remoteStream: null,
    isIncoming: false,
    status: 'idle',
    duration: 0
};

export const callStore = writable<CallState>(initialCallState);

class CallManager {
    private pc: RTCPeerConnection | null = null;
    private timerInterval: any = null;
    private iceCandidatesQueue: RTCIceCandidate[] = [];
    private config: RTCConfiguration = {
        iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]
    };

    async startCall(peerHash: string, type: 'voice' | 'video') {
        console.log(`Starting ${type} call to ${peerHash}`);
        this.iceCandidatesQueue = [];
        const stream = await navigator.mediaDevices.getUserMedia({
            audio: true,
            video: type === 'video' ? { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } } : false
        });

        const callId = crypto.randomUUID();
        callStore.set({
            isActive: true,
            peerHash,
            callId,
            type,
            localStream: stream,
            remoteStream: null,
            isIncoming: false,
            status: 'calling',
            duration: 0
        });

        this.pc = new RTCPeerConnection(this.config);

        this.pc.ontrack = (event) => {
            console.log(`Remote ${event.track.kind} track received`);
            callStore.update(s => ({ ...s, remoteStream: event.streams[0], status: 'connected' }));
            this.startTimer();
        };

        this.pc.onicecandidate = (event) => {
            if (event.candidate) {
                this.sendSignaling(peerHash, { type: 'ice-candidate', candidate: event.candidate });
            }
        };

        this.pc.oniceconnectionstatechange = () => {
            console.log(`ICE Connection State: ${this.pc?.iceConnectionState}`);
            if (this.pc?.iceConnectionState === 'failed') {
                console.warn("ICE Connection Failed - attempting restart...");
            }
        };

        stream.getTracks().forEach(track => {
            this.pc?.addTrack(track, stream);
        });

        const offer = await this.pc.createOffer();
        await this.pc.setLocalDescription(offer);

        this.sendSignaling(peerHash, { type: 'offer', sdp: offer, callType: type, callId });
    }

    private offerData: any = null;

    async handleSignaling(senderHash: string, data: any) {
        if (data.type === 'offer') {
            this.offerData = data;
            callStore.set({
                isActive: true,
                peerHash: senderHash,
                callId: data.callId,
                type: data.callType,
                localStream: null,
                remoteStream: null,
                isIncoming: true,
                status: 'ringing',
                duration: 0
            });
        } else if (data.type === 'answer') {
            await this.handleAnswer(data);
        } else if (data.type === 'ice-candidate') {
            await this.handleIceCandidate(data);
        } else if (data.type === 'hangup') {
            const state = get(callStore);
            if (state.status === 'ringing' || state.status === 'calling') {
                this.logCall('declined');
            } else if (state.status === 'connected') {
                this.logCall('completed');
            }
            this.endCall(false);
        }
    }

    async acceptCall() {
        const state = get(callStore);
        if (!state.peerHash || !this.offerData) return;

        this.iceCandidatesQueue = [];
        const stream = await navigator.mediaDevices.getUserMedia({
            audio: true,
            video: state.type === 'video' ? { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } } : false
        });

        callStore.update(s => ({ ...s, localStream: stream }));

        this.pc = new RTCPeerConnection(this.config);

        this.pc.ontrack = (event) => {
            console.log(`Remote ${event.track.kind} track received`);
            callStore.update(s => ({ ...s, remoteStream: event.streams[0], status: 'connected' }));
            this.startTimer();
        };

        this.pc.oniceconnectionstatechange = () => {
            console.log(`ICE Connection State: ${this.pc?.iceConnectionState}`);
        };

        this.pc.onicecandidate = (event) => {
            if (event.candidate) {
                this.sendSignaling(state.peerHash!, { type: 'ice-candidate', candidate: event.candidate });
            }
        };

        stream.getTracks().forEach(track => {
            this.pc?.addTrack(track, stream);
        });

        await this.pc.setRemoteDescription(new RTCSessionDescription(this.offerData.sdp));
        await this.processIceQueue();

        const answer = await this.pc.createAnswer();
        await this.pc.setLocalDescription(answer);

        this.sendSignaling(state.peerHash!, { type: 'answer', sdp: answer });
        this.offerData = null;
    }

    private async handleAnswer(data: any) {
        if (this.pc) {
            await this.pc.setRemoteDescription(new RTCSessionDescription(data.sdp));
            await this.processIceQueue();
        }
    }

    private async handleIceCandidate(data: any) {
        const candidate = new RTCIceCandidate(data.candidate);
        if (this.pc && this.pc.remoteDescription) {
            try {
                await this.pc.addIceCandidate(candidate);
            } catch (e) {
                console.warn("Error adding ICE candidate", e);
            }
        } else {
            this.iceCandidatesQueue.push(candidate);
        }
    }

    private async processIceQueue() {
        while (this.iceCandidatesQueue.length > 0) {
            const candidate = this.iceCandidatesQueue.shift();
            if (candidate && this.pc) {
                try {
                    await this.pc.addIceCandidate(candidate);
                } catch (e) {
                    console.warn("Error processing ICE queue", e);
                }
            }
        }
    }

    private startTimer() {
        if (this.timerInterval) return;
        this.timerInterval = setInterval(() => {
            callStore.update(s => ({ ...s, duration: s.duration + 1 }));
        }, 1000);
    }

    private stopTimer() {
        if (this.timerInterval) {
            clearInterval(this.timerInterval);
            this.timerInterval = null;
        }
    }

    async endCall(notifyPeer: boolean = true) {
        const state = get(callStore);
        if (notifyPeer && state.peerHash) {
            await this.sendSignaling(state.peerHash, { type: 'hangup' });

            if (state.status === 'connected') {
                this.logCall('completed');
            } else {
                this.logCall('declined');
            }

            
            await new Promise(r => setTimeout(r, 200));
        }

        state.localStream?.getTracks().forEach(track => track.stop());
        this.pc?.close();
        this.pc = null;
        this.stopTimer();
        this.offerData = null;
        this.iceCandidatesQueue = [];
        callStore.set(initialCallState);
    }

    private async sendSignaling(peerHash: string, data: any) {
        const serverUrl = get(userStore).relayUrl;
        const signalingObj = {
            type: 'signaling',
            data: data
        };
        const ciphertextObj = await signalManager.encrypt(peerHash, JSON.stringify(signalingObj), serverUrl, true);
        network.sendBinary(peerHash, new TextEncoder().encode(JSON.stringify(ciphertextObj)));
    }

    private async logCall(status: 'missed' | 'completed' | 'declined') {
        const state = get(callStore);
        if (!state.peerHash || !state.callId) return;

        const durationStr = state.duration > 0 ? ` (${Math.floor(state.duration / 60)}m ${state.duration % 60}s)` : "";
        const typeStr = state.type === 'video' ? 'Video' : 'Voice';
        const content = `${typeStr} Call ${status}${durationStr}`;

        const msg: Message = {
            id: state.callId,
            timestamp: Date.now(),
            senderHash: get(userStore).identityHash!,
            content: content,
            type: 'call_log',
            call_duration: state.duration,
            call_status: status,
            isMine: true,
            status: 'sent'
        };

        addMessage(state.peerHash, msg);

        const logSignal = {
            type: 'call_log',
            callId: state.callId,
            content: content,
            duration: state.duration,
            status: status === 'declined' ? 'missed' : status
        };
        const serverUrl = get(userStore).relayUrl;
        const ciphertextObj = await signalManager.encrypt(state.peerHash, JSON.stringify(logSignal), serverUrl, true);
        network.sendBinary(state.peerHash, new TextEncoder().encode(JSON.stringify(ciphertextObj)));
    }
}

export const callManager = new CallManager();
