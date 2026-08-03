import { useState, useEffect, useRef } from "react";
import { logger } from "@/lib/logger";

import { invoke } from "@tauri-apps/api/core";

export const useAudioPlayer = (audioPath: string | null) => {
    const [isPlaying, setIsPlaying] = useState(false);
    const [currentTime, setCurrentTime] = useState(0);
    const [duration, setDuration] = useState(0);
    const [error, setError] = useState<string | null>(null);
    const audioRef = useRef<AudioContext | null>(null);
    const sourceRef = useRef<AudioBufferSourceNode | null>(null);
    const startTimeRef = useRef<number>(0);
    const audioBufferRef = useRef<AudioBuffer | null>(null);
    const rafRef = useRef<number>();
    const seekTimeRef = useRef<number>(0);

    const initAudioContext = async () => {
        try {
            if (!audioRef.current) {
                logger.debug("Creating new AudioContext");
                const AudioContextClass = window.AudioContext || (window as any).webkitAudioContext;
                audioRef.current = new AudioContextClass();
                logger.debug("AudioContext created:", {
                    state: audioRef.current.state,
                    sampleRate: audioRef.current.sampleRate,
                });
            }

            if (audioRef.current.state === "suspended") {
                logger.debug("Resuming suspended AudioContext");
                await audioRef.current.resume();
                logger.debug("AudioContext resumed:", audioRef.current.state);
            }

            setError(null);
            return true;
        } catch (error) {
            logger.error("Error initializing AudioContext:", error);
            setError("Failed to initialize audio");
            return false;
        }
    };

    // Cleanup function
    useEffect(() => {
        return () => {
            logger.debug("Cleaning up audio resources");
            if (rafRef.current) {
                cancelAnimationFrame(rafRef.current);
            }
            if (sourceRef.current) {
                sourceRef.current.stop();
            }
            if (audioRef.current) {
                audioRef.current.close();
            }
        };
    }, []);

    const loadAudio = async () => {
        if (!audioPath) {
            logger.debug("No audio path provided");
            return;
        }

        try {
            // Initialize context first
            const initialized = await initAudioContext();
            if (!initialized || !audioRef.current) {
                logger.error("Failed to initialize audio context");
                return;
            }

            logger.debug("Loading audio from:", audioPath);

            // Read the file using Tauri command
            const result = await invoke<number[]>("read_audio_file", {
                filePath: audioPath,
            });

            if (!result || result.length === 0) {
                throw new Error("Empty audio data received");
            }

            logger.debug("Audio file read, size:", result.length, "bytes");

            // Create a copy of the audio data
            const audioData = new Uint8Array(result).buffer;

            logger.debug("Created audio buffer, size:", audioData.byteLength, "bytes");

            // Decode the audio data
            const audioBuffer = await new Promise<AudioBuffer>((resolve, reject) => {
                audioRef.current!.decodeAudioData(
                    audioData,
                    (buffer) => {
                        logger.debug("Audio decoded successfully:", {
                            duration: buffer.duration,
                            sampleRate: buffer.sampleRate,
                            numberOfChannels: buffer.numberOfChannels,
                            length: buffer.length,
                        });
                        resolve(buffer);
                    },
                    (error) => {
                        logger.error("Audio decoding failed:", error);
                        reject(new Error("Failed to decode audio data: " + error));
                    }
                );
            });

            audioBufferRef.current = audioBuffer;
            setDuration(audioBuffer.duration);
            setCurrentTime(0);
            setError(null);
            logger.debug("Audio loaded and ready to play");
        } catch (error) {
            logger.error("Error loading audio:", error);
            if (error instanceof Error) {
                logger.error("Error details:", {
                    message: error.message,
                    name: error.name,
                    stack: error.stack,
                });
            }
            setError("Failed to load audio file");
        }
    };

    // Load audio when path changes
    useEffect(() => {
        logger.debug("Audio path changed:", audioPath);
        if (audioPath) {
            loadAudio();
        }
    }, [audioPath]);

    const stopPlayback = () => {
        logger.debug("Stopping playback");
        if (rafRef.current) {
            cancelAnimationFrame(rafRef.current);
            rafRef.current = undefined;
        }
        if (sourceRef.current) {
            try {
                sourceRef.current.stop();
                sourceRef.current.disconnect();
            } catch (e) {
                logger.debug("Error stopping source:", e);
            }
            sourceRef.current = null;
        }
        setIsPlaying(false);
    };

    const play = async () => {
        logger.debug("Play requested");

        try {
            // Initialize context if needed
            const initialized = await initAudioContext();
            if (!initialized) {
                throw new Error("Audio context initialization failed");
            }
            if (!audioRef.current) {
                throw new Error("Audio context is null after initialization");
            }
            if (!audioBufferRef.current) {
                throw new Error("No audio buffer loaded - try loading the audio file first");
            }
            if (audioRef.current.state !== "running") {
                throw new Error(`Audio context is in invalid state: ${audioRef.current.state}`);
            }

            // Stop any existing playback
            stopPlayback();

            // Create and setup new source
            logger.debug("Creating new audio source");
            sourceRef.current = audioRef.current.createBufferSource();
            sourceRef.current.buffer = audioBufferRef.current;

            logger.debug("Audio buffer details:", {
                duration: audioBufferRef.current.duration,
                sampleRate: audioBufferRef.current.sampleRate,
                numberOfChannels: audioBufferRef.current.numberOfChannels,
                length: audioBufferRef.current.length,
            });

            sourceRef.current.connect(audioRef.current.destination);

            // Setup ended callback
            sourceRef.current.onended = () => {
                logger.debug("Playback ended naturally");
                stopPlayback();
                setCurrentTime(0);
            };

            // Start playback from the seek time
            const startTime = seekTimeRef.current;
            startTimeRef.current = audioRef.current.currentTime - startTime;
            logger.debug("Starting playback", {
                startTime,
                contextTime: audioRef.current.currentTime,
                seekTime: seekTimeRef.current,
            });

            sourceRef.current.start(0, startTime);
            setIsPlaying(true);
            setError(null);

            // Setup time update
            const updateTime = () => {
                if (!audioRef.current || !sourceRef.current) {
                    logger.debug("Update cancelled - context or source is null");
                    return;
                }

                const newTime = audioRef.current.currentTime - startTimeRef.current;

                if (newTime >= duration) {
                    logger.debug("Playback finished");
                    stopPlayback();
                    setCurrentTime(0);
                    seekTimeRef.current = 0;
                } else {
                    setCurrentTime(newTime);
                    seekTimeRef.current = newTime;
                    rafRef.current = requestAnimationFrame(updateTime);
                }
            };

            rafRef.current = requestAnimationFrame(updateTime);
        } catch (error) {
            logger.error("Error during playback:", error);
            setError("Failed to play audio");
            stopPlayback();
        }
    };

    const seek = async (time: number) => {
        logger.debug("Seek requested:", time);
        if (time < 0) time = 0;
        if (time > duration) time = duration;

        const wasPlaying = isPlaying;

        // Stop current playback
        stopPlayback();

        // Update both current time and seek time reference
        seekTimeRef.current = time;
        setCurrentTime(time);

        // If it was playing before, restart playback at new position
        if (wasPlaying) {
            logger.debug("Restarting playback at:", time);
            await play();
        }
    };

    const pause = () => {
        logger.debug("Pause requested");
        stopPlayback();
    };

    return {
        isPlaying,
        currentTime,
        duration,
        error,
        play,
        pause,
        seek,
    };
};
