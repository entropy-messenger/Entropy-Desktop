
/**
 * Represents an individual message in a conversation.
 * Handles both plain text and encrypted media attachments.
 */
export interface Message {
    id: string;
    timestamp: number;
    senderHash: string;
    senderAlias?: string;
    content: string;
    type: 'text' | 'voice_note' | 'file' | 'signaling' | 'system';
    attachment?: {
        url?: string;
        fileName?: string;
        fileType?: string;
        size?: number;
        data?: Uint8Array;
        isDownloaded?: boolean;
        exportedPath?: string;
        originalPath?: string;
        bundle?: any;
    };
    groupId?: string;
    isMine: boolean;
    status: 'sending' | 'sent' | 'delivered' | 'read' | 'failed';
    isStarred?: boolean;
    isGroup?: boolean;
    chatAlias?: string;
    chatMembers?: string[];
    chatAddress?: string;
    replyTo?: {
        id: string;
        content: string;
        senderHash?: string;
        senderAlias?: string;
        type: Message['type'];
    };
}

/**
 * Security and visibility configurations for the local identity.
 */
export interface PrivacySettings {
    readReceipts: boolean;
    typingStatus: 'everyone' | 'nobody';
    routingMode: 'direct' | 'tor' | 'custom';
    proxyUrl?: string;
    theme?: 'light' | 'dark';
}

/**
 * Represents a conversation thread with a single peer or a group.
 */
export interface Chat {
    peerHash: string;
    messages?: Message[];
    hasMore?: boolean;
    unreadCount: number,
    isTyping?: boolean,
    isGroup?: boolean;
    members?: string[];
    isPinned?: boolean;
    isArchived?: boolean;
    isBlocked?: boolean;
    trustLevel: number;
    localNickname?: string;
    globalNickname?: string;
    peerNickname?: string;
    isVerified?: boolean;
    inviteCode?: string;
    lastMsg?: string;
    lastTimestamp?: number;
    lastStatus?: Message['status'];
    lastIsMine?: boolean;
    lastSenderHash?: string;
    lastIdentityCheck?: number;
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
