
export interface KeyPair {
    publicKey: Uint8Array;
    privateKey: Uint8Array;
    keyType: 'ed25519' | 'x25519';
}

export interface UserIdentity {
    identityKey: KeyPair;
    ephemeralKey: KeyPair;
    identityHash: string;
    publicKeyHex: string;
    mnemonic?: string;
}

export interface LinkPreview {
    url: string;
    title?: string;
    description?: string;
    siteName?: string;
    image?: string;
}

export interface Message {
    id: string;
    timestamp: number;
    senderHash: string;
    senderAlias?: string;
    content: string;
    type: 'text' | 'voice_note' | 'file' | 'call_log' | 'signaling';
    attachment?: {
        url?: string;
        fileName?: string;
        fileType?: string;
        size?: number;
        data?: Uint8Array;
    };
    call_duration?: number;
    call_status?: 'missed' | 'completed' | 'declined';
    groupId?: string;
    isMine: boolean;
    status: 'sending' | 'sent' | 'delivered' | 'read' | 'failed';
    pfp?: string;
    isStarred?: boolean;
    replyTo?: {
        id: string;
        content: string;
        senderAlias?: string;
        type: Message['type'];
    };
    linkPreview?: LinkPreview;
}

export interface PrivacySettings {
    readReceipts: boolean;
    lastSeen: 'everyone' | 'nobody';
    profilePhoto: 'everyone' | 'nobody';
    routingMode: 'direct' | 'tor' | 'custom';
    proxyUrl?: string;
    decoyMode: boolean;
}

export interface Chat {
    peerHash: string;
    peerAlias?: string;
    pfp?: string;
    messages: Message[];
    lastSeen?: number;
    unreadCount: number;
    isOnline?: boolean;
    isTyping?: boolean;
    isGroup?: boolean;
    members?: string[];
    isPinned?: boolean;
    isArchived?: boolean;
    isMuted?: boolean;
    isVerified?: boolean;
    disappearingTimer?: number;
    localNickname?: string;
    inviteCode?: string;
}

export interface ServerMessage {
    type: string | number;
    payload?: any;
    [key: string]: any;
}

export interface AuthPayload {
    timestamp: number;
    nonce: number;
    identity_hash: string;
}
