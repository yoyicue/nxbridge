use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use async_shutdown::ShutdownManager;
use rtsp_types::Method;
use rtsp_types::headers;
use rtsp_types::headers::Transport;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use crate::ShutdownReason;
use crate::session::manager::SessionManager;
use crate::session::stream::audio::ALL_AUDIO_CONFIGS;
use crate::session::stream::audio::AudioChannels;
use crate::session::stream::audio::AudioConfig;
use crate::session::stream::audio::AudioStreamConfig;
use crate::session::stream::audio::AudioStreamContext;
use crate::session::stream::control::ControlStreamConfig;
use crate::session::stream::video::VideoChromaSampling;
use crate::session::stream::video::VideoDynamicRange;
use crate::session::stream::video::VideoFormat;
use crate::session::stream::video::VideoStreamConfig;
use crate::session::stream::video::VideoStreamContext;

// Adapted from Moonshine commits 3725d143, e0324dc6, 38db59c5 and
// 631e359e. Preserve bounds across all partial reads of one request.
const MAX_RTSP_REQUEST_BYTES: usize = 64 * 1024;
const RTSP_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn rtsp_request_fits(current: usize, incoming: usize) -> bool {
	current
		.checked_add(incoming)
		.is_some_and(|size| size <= MAX_RTSP_REQUEST_BYTES)
}

fn video_bitrate_bps(kbps: usize) -> Option<usize> {
	kbps.checked_mul(1_000).filter(|bps| u32::try_from(*bps).is_ok())
}

fn replace_bytes(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
	let mut result = Vec::with_capacity(haystack.len());
	let mut rest = haystack;
	while let Some(position) = rest.windows(from.len()).position(|window| window == from) {
		result.extend_from_slice(&rest[..position]);
		result.extend_from_slice(to);
		rest = &rest[position + from.len()..];
	}
	result.extend_from_slice(rest);
	result
}

async fn read_rtsp_chunk<R: tokio::io::AsyncRead + Unpin>(
	reader: &mut R,
	buffer: &mut [u8],
	deadline: tokio::time::Instant,
) -> std::io::Result<usize> {
	tokio::time::timeout_at(deadline, reader.read(buffer))
		.await
		.map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "RTSP request deadline exceeded"))?
}

#[repr(u8)]
enum ServerCapabilities {
	_TouchEvents = 0x01,
	ControllerTouchEvents = 0x02,
}

#[repr(u8)]
enum EncryptionFlags {
	ControlV2 = 0x01,
	Video = 0x02,
	Audio = 0x04,
}

#[derive(Clone)]
pub struct RtspServer {
	address: String,
	rtsp_port: u16,
	video_config: VideoStreamConfig,
	audio_config: AudioStreamConfig,
	control_config: ControlStreamConfig,
	session_manager: SessionManager,
}

impl RtspServer {
	pub fn new(
		address: String,
		rtsp_port: u16,
		video_config: VideoStreamConfig,
		audio_config: AudioStreamConfig,
		control_config: ControlStreamConfig,
		session_manager: SessionManager,
		shutdown: ShutdownManager<ShutdownReason>,
	) -> Self {
		let server = Self {
			address,
			rtsp_port,
			video_config: video_config.clone(),
			audio_config: audio_config.clone(),
			control_config: control_config.clone(),
			session_manager,
		};

		tokio::spawn({
			let server = server.clone();
			async move {
				let _ = shutdown
					.wrap_cancel(shutdown.wrap_trigger_shutdown(ShutdownReason::RtspShutdown, {
						let server = server.clone();
						async move {
							let ip = server
								.address
								.parse::<IpAddr>()
								.map_err(|e| tracing::error!("Failed to parse address '{}': {}", server.address, e))?;
							let socket_addr = SocketAddr::new(ip, server.rtsp_port);
							let listener = TcpListener::bind(socket_addr)
								.await
								.map_err(|e| tracing::error!("Failed to bind to address {}: {}", socket_addr, e))?;

							tracing::debug!("RTSP server listening on {}", socket_addr);

							loop {
								let (connection, address) = listener
									.accept()
									.await
									.map_err(|e| tracing::error!("Failed to accept connection: {}", e))?;
								tracing::trace!("Accepted connection from {}", address);

								tokio::spawn({
									let server = server.clone();
									async move {
										let _ = server.handle_connection(connection, address).await;
									}
								});
							}

							// Is there another way to define the return type of this function?
							#[allow(unreachable_code)]
							Ok::<(), ()>(())
						}
					}))
					.await;

				tracing::debug!("RTSP server shutting down.");
			}
		});

		server
	}

	fn capabilities(&self) -> u8 {
		ServerCapabilities::ControllerTouchEvents as u8
	}

	fn encryption_flags_supported(&self) -> u8 {
		let mut flags = EncryptionFlags::ControlV2 as u8 | EncryptionFlags::Audio as u8;
		if self.video_config.encrypt {
			flags |= EncryptionFlags::Video as u8;
		}
		flags
	}

	#[allow(clippy::result_unit_err)]
	pub fn description(&self) -> String {
		// This is a very simple SDP description, the minimal that Moonlight requires.
		// TODO: Fill this based on server settings.
		// TODO: Use:
		//       "a=x-ss-general.featureFlags: <FEATURE FLAGS>"
		//       "x-nv-video[0].refPicInvalidation=1"
		//       "a=rtpmap:98 AV1/90000" (For AV1 support)
		//       "a=fmtp:97 surround-params=<SURROUND PARAMS>"
		//       "<AUDIO STREAM MAPPING>"
		let mut result = String::new();

		result.push_str(&format!("a=x-ss-general.featureFlags:{}\n", self.capabilities()));
		result.push_str(&format!(
			"a=x-ss-general.encryptionSupported:{}\n",
			self.encryption_flags_supported()
		));
		// NOT advertised: "sprop-parameter-sets=AAAAAU". That string is the
		// base64 prefix of an HEVC VPS NALU, and moonlight-common-c decides the
		// host supports HEVC purely by string-matching it in this payload
		// (RtspConnection.c: `strstr(response.payload, "sprop-parameter-sets=AAAAAU")`).
		// It does NOT consult ServerCodecModeSupport for that decision, so
		// advertising H.264-only there is not enough. Upstream emitted this
		// because its Vulkan encoder really did do HEVC; Cedar here does not,
		// and a client that picks HEVC gets an H.264 stream it will never
		// decode - it waits for an HEVC VPS that never arrives and reports
		// only a generic connection error, with nothing logged on either side.
		// The Vulkan encoder this was written for could invalidate individual
		// reference frames on request; Cedar (the encoder this project
		// actually drives) exposes no such API, so advertising support here
		// would make Moonlight send InvalidateReferenceFrames instead of
		// RequestIdrFrame on packet loss — a request we can't act on any
		// more cheaply than a full IDR. Advertise the honest capability.
		result.push_str("a=x-nv-video[0].refPicInvalidation:0\n");
		// Likewise omitted: "a=rtpmap:98 AV1/90000". The same negotiation
		// selects AV1 whenever the payload contains "AV1/90000" and the client
		// has AV1 hardware, which would fail the same silent way.
		result.push_str("a=fmtp:96 packetization-mode=1\n");

		// Emit surround-params for each Opus configuration.
		// Moonlight selects the appropriate config at ANNOUNCE based on channel count and quality.
		// Values are packed as single digits with no separators, matching moonlight-common-c's
		// parseOpusConfigFromParamString() which reads one digit character at a time.
		for (i, config) in ALL_AUDIO_CONFIGS.iter().enumerate() {
			// GFE advertises an incorrect mapping for normal quality surround configurations,
			// rotating LFE (index 3) to the end. Moonlight undoes this rotation after parsing.
			// We must apply the same rotation for normal quality 5.1 and 7.1 configs.
			let mut mapping = config.mapping;
			let is_normal_surround = i == 2 || i == 4; // SURROUND51 or SURROUND71
			if is_normal_surround && config.channels >= AudioChannels::Surround51 {
				mapping[3..config.channels as usize].rotate_left(1);
			}

			let mut params = format!(
				"{}{}{}",
				config.channels as usize, config.streams, config.coupled_streams
			);
			for &m in mapping.iter().take(config.channels as usize) {
				params.push_str(&format!("{}", m));
			}
			result.push_str(&format!("a=fmtp:97 surround-params={}\n", params));
		}

		result
	}

	fn handle_options_request(
		&self,
		request: &rtsp_types::Request<Vec<u8>>,
		cseq: i32,
	) -> rtsp_types::Response<Vec<u8>> {
		rtsp_types::Response::builder(request.version(), rtsp_types::StatusCode::Ok)
			.header(headers::CSEQ, cseq.to_string())
			.header(headers::PUBLIC, "OPTIONS DESCRIBE SETUP PLAY")
			.build(Vec::new())
	}

	fn handle_setup_request(&self, request: &rtsp_types::Request<Vec<u8>>, cseq: i32) -> rtsp_types::Response<Vec<u8>> {
		let transports = match request.typed_header::<rtsp_types::headers::Transports>() {
			Ok(transports) => transports,
			Err(e) => {
				tracing::warn!("Failed to parse transport information from SETUP request: {e}");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let transports = match transports {
			Some(transports) => transports,
			None => {
				tracing::warn!("No transport information in SETUP request.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};

		if let Some(transport) = (*transports).first() {
			match transport {
				Transport::Other(_transport) => {
					let request_uri = match request.request_uri() {
						Some(query) => query,
						None => {
							tracing::warn!("No request URI in SETUP request.");
							return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
						},
					};
					let query = match request_uri.query_pairs().next() {
						Some(query) => query,
						None => {
							tracing::warn!("No query in request URI in SETUP request.");
							return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
						},
					};
					if query.0 != "streamid" {
						tracing::warn!("Expected only one query parameter with 'streamid', but didn't find it.");
						return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
					}

					// Example query: streamid=control/13/0
					let (stream_id, port) = match query.1.split('/').next() {
						Some("video") => ("video", self.video_config.port),
						Some("audio") => ("audio", self.audio_config.port),
						Some("control") => ("control", self.control_config.port),
						Some(stream) => {
							tracing::warn!("Unknown stream '{stream}'");
							return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
						},
						None => {
							tracing::warn!("Unexpected query format for query '{}'", query.1);
							return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
						},
					};

					tracing::debug!("Responding with server_port={port} for stream '{stream_id}'.");

					return rtsp_types::Response::builder(request.version(), rtsp_types::StatusCode::Ok)
						.header(headers::CSEQ, cseq.to_string())
						.header(headers::SESSION, "MoonshineSession;timeout = 90".to_string())
						.header(headers::TRANSPORT, format!("server_port={port}"))
						.build(Vec::new());
				},
				t => {
					tracing::warn!("Received request for unsupported transport: {:?}", t);
					return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
				},
			}
		}

		tracing::warn!("No transports found in SETUP request.");
		rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest)
	}

	async fn handle_describe_request(
		&self,
		request: &rtsp_types::Request<Vec<u8>>,
		cseq: i32,
	) -> rtsp_types::Response<Vec<u8>> {
		let description = self.description();
		tracing::debug!("SDP session data: \n{}", description.trim());
		rtsp_types::Response::builder(request.version(), rtsp_types::StatusCode::Ok)
			.header(headers::CSEQ, cseq.to_string())
			.build(description.into_bytes())
	}

	async fn handle_announce_request(
		&self,
		request: &rtsp_types::Request<Vec<u8>>,
		cseq: i32,
	) -> rtsp_types::Response<Vec<u8>> {
		let sdp_session = match sdp_types::Session::parse(request.body()) {
			Ok(sdp_session) => sdp_session,
			Err(e) => {
				tracing::warn!("Failed to parse ANNOUNCE request as SDP session: {e}");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};

		tracing::trace!("Received SDP session from ANNOUNCE request: {sdp_session:#?}");

		let width = match get_sdp_attribute(&sdp_session, "x-nv-video[0].clientViewportWd") {
			Ok(width) => width,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-video[0].clientViewportWd in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let height = match get_sdp_attribute(&sdp_session, "x-nv-video[0].clientViewportHt") {
			Ok(height) => height,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-video[0].clientViewportHt in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let fps: u32 = match get_sdp_attribute(&sdp_session, "x-nv-video[0].maxFPS") {
			Ok(fps) => fps,
			Err(()) => {
				tracing::warn!("Failed to parse xx-nv-video[0].maxFPS in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		if fps == 0 {
			tracing::warn!("Rejected zero client video frame rate");
			return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
		}
		let packet_size: usize = match get_sdp_attribute(&sdp_session, "x-nv-video[0].packetSize") {
			Ok(packet_size) => packet_size,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-video[0].packetSize in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		if !crate::session::stream::video::valid_packet_size(packet_size) {
			tracing::warn!("Rejected invalid client video packet size: {packet_size}");
			return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
		}
		let bitrate_kbps: usize = match get_sdp_attribute(&sdp_session, "x-ml-video.configuredBitrateKbps") {
			Ok(bitrate) => bitrate,
			Err(()) => {
				tracing::warn!("Failed to parse x-ml-video.configuredBitrateKbps in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let Some(bitrate) = video_bitrate_bps(bitrate_kbps) else {
			tracing::warn!("Client video bitrate overflows the encoder: {bitrate_kbps} kbps");
			return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
		};
		let minimum_fec_packets = match get_sdp_attribute(&sdp_session, "x-nv-vqos[0].fec.minRequiredFecPackets") {
			Ok(minimum_fec_packets) => minimum_fec_packets,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-vqos[0].fec.minRequiredFecPackets in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let video_qos_type: String = match get_sdp_attribute(&sdp_session, "x-nv-vqos[0].qosTrafficType") {
			Ok(video_qos_type) => video_qos_type,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-vqos[0].qosTrafficType in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let video_format: u32 = match get_sdp_attribute(&sdp_session, "x-nv-vqos[0].bitStreamFormat") {
			Ok(video_format) => video_format,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-vqos[0].bitStreamFormat in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let video_format = match VideoFormat::try_from(video_format) {
			Ok(video_format) => video_format,
			Err(()) => {
				tracing::warn!("Invalid video format: {}", video_format);
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};

		let dynamic_range: u32 =
			get_optional_sdp_attribute(&sdp_session, "x-nv-video[0].dynamicRangeMode").unwrap_or_default();
		let dynamic_range = VideoDynamicRange::try_from(dynamic_range).unwrap_or_default();

		let chroma_sampling_type: u32 =
			get_optional_sdp_attribute(&sdp_session, "x-ss-video[0].chromaSamplingType").unwrap_or_default();
		let chroma_sampling_type = VideoChromaSampling::try_from(chroma_sampling_type).unwrap_or_default();

		let max_reference_frames: u32 =
			get_optional_sdp_attribute(&sdp_session, "x-nv-video[0].maxNumReferenceFrames").unwrap_or(1);

		// Bit 0 selects full-range luma, the remaining bits an SDR colorspace.
		// Sunshine: `colorspace_from_client_config()`
		const CSC_COLORSPACE_REC601: u32 = 0;
		const CSC_COLORSPACE_REC709: u32 = 1;
		let encoder_csc_mode: Option<u32> = get_optional_sdp_attribute(&sdp_session, "x-nv-video[0].encoderCscMode");
		let full_range = encoder_csc_mode.unwrap_or_default() & 0x1 != 0;

		// Only Rec.709 is encoded. Rec.601 doubles as the protocol default for
		// clients that never chose a colorspace, so it stays quiet; higher
		// values are explicit requests worth a warning.
		match encoder_csc_mode.map(|mode| mode >> 1) {
			Some(CSC_COLORSPACE_REC601) => {
				tracing::debug!("Client sent SDR colorspace Rec.601 (the protocol default), encoding Rec.709.");
			},
			Some(colorspace) if colorspace != CSC_COLORSPACE_REC709 => {
				tracing::warn!(
					"Client requested SDR colorspace {colorspace} via encoderCscMode, encoding Rec.709 instead."
				);
			},
			_ => {},
		}
		tracing::debug!(
			"Client requested {} range video.",
			if full_range { "full" } else { "limited" }
		);

		// Parse the client's encryption flags from the ANNOUNCE SDP.
		let client_encryption_flags: u8 =
			get_optional_sdp_attribute(&sdp_session, "x-ss-general.encryptionEnabled").unwrap_or(0);

		let video_stream_context = VideoStreamContext {
			width,
			height,
			fps,
			packet_size,
			bitrate,
			minimum_fec_packets,
			qos: video_qos_type != "0",
			video_format,
			dynamic_range,
			chroma_sampling_type,
			max_reference_frames,
			full_range,
			encrypt_video: self.video_config.encrypt && (client_encryption_flags & EncryptionFlags::Video as u8 != 0),
		};

		let packet_duration: u32 = match get_sdp_attribute(&sdp_session, "x-nv-aqos.packetDuration") {
			Ok(packet_duration) => packet_duration,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-aqos.packetDuration in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};
		let audio_qos_type: String = match get_sdp_attribute(&sdp_session, "x-nv-aqos.qosTrafficType") {
			Ok(audio_qos_type) => audio_qos_type,
			Err(()) => {
				tracing::warn!("Failed to parse x-nv-aqos.qosTrafficType in SDP session.");
				return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
			},
		};

		// Parse surround audio attributes from SDP.
		let surround_channels: u8 =
			get_optional_sdp_attribute(&sdp_session, "x-nv-audio.surround.numChannels").unwrap_or(2);
		let surround_mask: u32 =
			get_optional_sdp_attribute(&sdp_session, "x-nv-audio.surround.channelMask").unwrap_or(0x3);
		let surround_enable: u8 = get_optional_sdp_attribute(&sdp_session, "x-nv-audio.surround.enable").unwrap_or(0);
		let audio_quality: u8 =
			get_optional_sdp_attribute(&sdp_session, "x-nv-audio.surround.AudioQuality").unwrap_or(1);

		let (channels, channel_mask) = if surround_enable != 0 {
			(AudioChannels::from(surround_channels), surround_mask)
		} else {
			// Fall back to the values from the HTTP launch request.
			let ctx = self.session_manager.get_session_context().await;
			match ctx {
				Ok(Some(session_ctx)) => (session_ctx.audio_channels, session_ctx.audio_channel_mask),
				_ => (AudioChannels::Stereo, 0x3),
			}
		};
		let high_quality = audio_quality != 0;
		let audio_config = AudioConfig::from_channels(channels, channel_mask, high_quality);
		if packet_duration != 5 || !matches!(audio_config.channels, AudioChannels::Stereo) {
			tracing::warn!("NXBridge synthetic source supports only 5 ms stereo audio packets");
			return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest);
		}

		tracing::debug!(
			"Audio config: {} channels, mask=0x{:x}, high_quality={}, config={:?}",
			audio_config.channels,
			audio_config.channel_mask,
			audio_config.high_quality,
			audio_config.stream_config
		);

		let audio_stream_context = AudioStreamContext {
			packet_duration_ms: packet_duration,
			qos: audio_qos_type != "0",
			audio_config,
			encrypt_audio: client_encryption_flags & EncryptionFlags::Audio as u8 != 0,
		};

		tracing::info!(
			"Negotiated encryption: control_v2={}, video={}, audio={}; video FEC={}%, audio packet={}ms",
			client_encryption_flags & EncryptionFlags::ControlV2 as u8 != 0,
			video_stream_context.encrypt_video,
			audio_stream_context.encrypt_audio,
			self.video_config.fec_percentage,
			packet_duration
		);
		if self
			.session_manager
			.set_stream_context(video_stream_context, audio_stream_context)
			.await
			.is_err()
		{
			return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::InternalServerError);
		}

		rtsp_types::Response::builder(request.version(), rtsp_types::StatusCode::Ok)
			.header(headers::CSEQ, cseq.to_string())
			.build(Vec::new())
	}

	async fn handle_play_request(
		&self,
		request: &rtsp_types::Request<Vec<u8>>,
		cseq: i32,
	) -> rtsp_types::Response<Vec<u8>> {
		if self.session_manager.start_session().await.is_err() {
			return rtsp_response(cseq, request.version(), rtsp_types::StatusCode::InternalServerError);
		}

		rtsp_types::Response::builder(request.version(), rtsp_types::StatusCode::Ok)
			.header(headers::CSEQ, cseq.to_string())
			.build(Vec::new())
	}

	async fn handle_connection(&self, mut connection: TcpStream, address: SocketAddr) -> Result<(), ()> {
		// Gate RTSP access: only process requests when a session has been initialized
		// via the authenticated /launch or /resume endpoint (H-3 mitigation).
		match self.session_manager.get_session_context().await {
			Ok(Some(_)) => {},
			_ => {
				tracing::warn!("Rejected RTSP connection from {}: no active session", address);
				return Ok(());
			},
		}

		let mut message_buffer = Vec::new();
		let deadline = tokio::time::Instant::now() + RTSP_REQUEST_TIMEOUT;

		let message = loop {
			let mut buffer = [0u8; 2048];
			let bytes_read = read_rtsp_chunk(&mut connection, &mut buffer, deadline)
				.await
				.map_err(|e| tracing::warn!("Failed to read from connection '{}': {}", address, e))?;
			if bytes_read == 0 {
				tracing::warn!("Received empty RTSP request.");
				return Ok(());
			}
			if !rtsp_request_fits(message_buffer.len(), bytes_read) {
				tracing::warn!("Rejected RTSP request over {MAX_RTSP_REQUEST_BYTES} bytes from {address}");
				return Err(());
			}
			message_buffer.extend_from_slice(&buffer[..bytes_read]);

			// Hacky workaround to fix rtsp_types parsing SETUP/PLAY requests from Moonlight.
			let message_buffer = replace_bytes(&message_buffer, b"streamid", b"rtsp://localhost?streamid");
			let message_buffer = replace_bytes(&message_buffer, b"PLAY /", b"PLAY rtsp://localhost/");

			tracing::trace!("Request: {}", String::from_utf8_lossy(&message_buffer));
			let result = rtsp_types::Message::parse(&message_buffer);

			break match result {
				Ok((message, _consumed)) => message,
				Err(rtsp_types::ParseError::Incomplete(_)) => {
					tracing::debug!("Incomplete RTSP message received, waiting for more data.");
					continue;
				},
				Err(e) => {
					tracing::warn!("Failed to parse request as RTSP message: {}", e);
					return Err(());
				},
			};
		};

		// tracing::trace!("Consumed {} bytes into RTSP request: {:#?}", consumed, message);

		let response = match message {
			rtsp_types::Message::Request(ref request) => {
				tracing::debug!("Received RTSP {:?} request", request.method());

				let cseq: i32 = request
					.header(&headers::CSEQ)
					.ok_or_else(|| tracing::warn!("RTSP request has no CSeq header"))?
					.as_str()
					.parse()
					.map_err(|e| tracing::warn!("Failed to parse CSeq header: {}", e))?;

				match request.method() {
					Method::Announce => self.handle_announce_request(request, cseq).await,
					Method::Describe => self.handle_describe_request(request, cseq).await,
					Method::Options => self.handle_options_request(request, cseq),
					Method::Setup => self.handle_setup_request(request, cseq),
					Method::Play => self.handle_play_request(request, cseq).await,
					method => {
						tracing::warn!("Received request with unsupported method {:?}", method);
						rtsp_response(cseq, request.version(), rtsp_types::StatusCode::BadRequest)
					},
				}
			},
			_ => {
				tracing::warn!("Unknown RTSP message type received");
				rtsp_response(0, rtsp_types::Version::V2_0, rtsp_types::StatusCode::BadRequest)
			},
		};

		tracing::debug!("Sending RTSP response");
		tracing::trace!("{:#?}", response);

		let mut buffer = Vec::new();
		response
			.write(&mut buffer)
			.map_err(|e| tracing::warn!("Failed to serialize RTSP response: {}", e))?;

		connection
			.write_all(&buffer)
			.await
			.map_err(|e| tracing::warn!("Failed to send RTSP response: {}", e))?;

		// For some reason, Moonlight expects a connection per request, so we close the connection here.
		connection
			.shutdown()
			.await
			.map_err(|e| tracing::warn!("Failed to shutdown the connection: {e}"))?;

		Ok(())
	}
}

fn rtsp_response(
	cseq: i32,
	version: rtsp_types::Version,
	status: rtsp_types::StatusCode,
) -> rtsp_types::Response<Vec<u8>> {
	rtsp_types::Response::builder(version, status)
		.header(headers::CSEQ, cseq.to_string())
		.build(Vec::new())
}

fn get_optional_sdp_attribute<F: FromStr>(sdp_session: &sdp_types::Session, attribute: &str) -> Option<F> {
	sdp_session
		.get_first_attribute_value(attribute)
		.ok()
		.flatten()
		.map(|s| s.trim())
		.and_then(|s| s.parse().ok())
}

fn get_sdp_attribute<F: FromStr>(sdp_session: &sdp_types::Session, attribute: &str) -> Result<F, ()> {
	sdp_session
		.get_first_attribute_value(attribute)
		.map_err(|e| tracing::warn!("Failed to attribute {attribute} from request: {e}"))?
		.ok_or_else(|| tracing::warn!("No {attribute} attribute in request"))?
		.trim()
		.parse()
		.map_err(|_| tracing::warn!("Attribute {attribute} can't be parsed."))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn bounds_and_byte_normalization() {
		assert!(rtsp_request_fits(MAX_RTSP_REQUEST_BYTES - 2048, 2048));
		assert!(!rtsp_request_fits(MAX_RTSP_REQUEST_BYTES, 1));
		assert!(!rtsp_request_fits(usize::MAX, 1));
		assert_eq!(
			replace_bytes(&[0xff, b'P', b'L', b'A', b'Y', b' ', b'/'], b"PLAY /", b"X"),
			[0xff, b'X']
		);
		assert_eq!(
			replace_bytes(b"streamid=0", b"streamid", b"rtsp://localhost?streamid"),
			b"rtsp://localhost?streamid=0"
		);
		assert_eq!(video_bitrate_bps(5_000), Some(5_000_000));
		assert_eq!(video_bitrate_bps((u32::MAX as usize) / 1_000 + 1), None);
		assert_eq!(video_bitrate_bps(usize::MAX), None);
	}
	#[tokio::test]
	async fn fragmented_rtsp_request_reaches_real_handler() {
		use crate::session::{SessionContext, SessionKeys};
		let shutdown = ShutdownManager::new();
		let video = VideoStreamConfig {
			port: 0,
			..Default::default()
		};
		let audio = AudioStreamConfig { port: 0 };
		let control = ControlStreamConfig { port: 0 };
		let manager = SessionManager::new(
			video.clone(),
			audio.clone(),
			control.clone(),
			"127.0.0.1".into(),
			5,
			shutdown,
		)
		.unwrap();
		manager
			.initialize_session(SessionContext {
				application_id: 1,
				resolution: (1280, 720),
				refresh_rate: 30,
				keys: SessionKeys::new(vec![0x42; 16], 1),
				audio_channels: AudioChannels::Stereo,
				audio_channel_mask: 3,
				hdr: false,
			})
			.await
			.unwrap();
		let server = RtspServer {
			address: "127.0.0.1".into(),
			rtsp_port: 0,
			video_config: video,
			audio_config: audio,
			control_config: control,
			session_manager: manager.clone(),
		};
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let mut client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
		let (peer, address) = listener.accept().await.unwrap();
		let handler = server.clone();
		let task = tokio::spawn(async move { server.handle_connection(peer, address).await });
		let message = "OPTIONS rtsp://localhost/ RTSP/1.0\r\nCSeq: 7\r\nX-Name: 中文\r\n\r\n".as_bytes();
		let split = message.iter().position(|b| *b >= 0x80).unwrap() + 1;
		client.write_all(&message[..split]).await.unwrap();
		tokio::time::sleep(std::time::Duration::from_millis(20)).await;
		client.write_all(&message[split..]).await.unwrap();
		let mut response = Vec::new();
		tokio::time::timeout(std::time::Duration::from_secs(3), client.read_to_end(&mut response))
			.await
			.unwrap()
			.unwrap();
		assert!(String::from_utf8_lossy(&response).starts_with("RTSP/1.0 200"));
		assert!(String::from_utf8_lossy(&response).contains("CSeq: 7"));
		task.await.unwrap().unwrap();
		let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
		let mut client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
		let (peer, address) = listener.accept().await.unwrap();
		let overflow_handler = handler.clone();
		let overflow = tokio::spawn(async move { overflow_handler.handle_connection(peer, address).await });
		let mut request = b"OPTIONS rtsp://localhost/ RTSP/1.0\r\nCSeq: 7\r\nX-Fill: ".to_vec();
		request.resize(MAX_RTSP_REQUEST_BYTES + 2048, b'a');
		let _ = client.write_all(&request).await; // Rejection may close the socket during the write.
		assert!(
			tokio::time::timeout(std::time::Duration::from_secs(3), overflow)
				.await
				.unwrap()
				.unwrap()
				.is_err()
		);
		manager.launch_session().await.unwrap();
		let announce = |fps: &str, packet: &str, bitrate: &str| {
			let sdp = format!(
				"v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=GameStream\r\nt=0 0\r\n\
                a=x-nv-video[0].clientViewportWd:1280\r\na=x-nv-video[0].clientViewportHt:720\r\n\
                a=x-nv-video[0].maxFPS:{fps}\r\na=x-nv-video[0].packetSize:{packet}\r\n\
                a=x-ml-video.configuredBitrateKbps:{bitrate}\r\na=x-nv-vqos[0].fec.minRequiredFecPackets:0\r\n\
                a=x-nv-vqos[0].qosTrafficType:0\r\na=x-nv-vqos[0].bitStreamFormat:0\r\n\
                a=x-nv-aqos.packetDuration:5\r\na=x-nv-aqos.qosTrafficType:0\r\n"
			);
			rtsp_types::Request::builder(Method::Announce, rtsp_types::Version::V1_0).build(sdp.into_bytes())
		};
		for req in [
			announce("0", "1400", "5000"),
			announce("30", "16", "5000"),
			announce("30", "1400", &usize::MAX.to_string()),
		] {
			assert_eq!(
				handler.handle_announce_request(&req, 8).await.status(),
				rtsp_types::StatusCode::BadRequest
			);
		}
		assert_eq!(
			handler
				.handle_announce_request(&announce("30", "1400", "5000"), 9)
				.await
				.status(),
			rtsp_types::StatusCode::Ok
		);
		manager.stop_session().await.unwrap();
	}
	#[tokio::test]
	async fn partial_reads_share_one_deadline() {
		let (mut reader, mut writer) = tokio::io::duplex(16);
		writer.write_all(b"RT").await.unwrap();
		let mut buffer = [0u8; 16];
		let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(60);
		assert_eq!(read_rtsp_chunk(&mut reader, &mut buffer, deadline).await.unwrap(), 2);
		assert_eq!(
			read_rtsp_chunk(&mut reader, &mut buffer, deadline)
				.await
				.unwrap_err()
				.kind(),
			std::io::ErrorKind::TimedOut
		);
	}
}
