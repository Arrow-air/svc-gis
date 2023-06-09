//! This module contains functions for routing between nodes.
use crate::grpc::server::grpc_server::{BestPathRequest, PathSegment};
use chrono::{DateTime, Utc};
use lib_common::time::timestamp_to_datetime;
use uuid::Uuid;

// TODO(R4): Include altitude, lanes, corridors
const ALTITUDE_HARDCODE: f32 = 1000.0;

/// Routing can occur from a vertiport to a vertiport
/// Or an aircraft to a vertiport (in-flight re-routing)
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PathType {
    /// Route between vertiports
    PortToPort = 0,

    /// Route from an aircraft to a vertiport
    AircraftToPort = 1,
}

/// Possible errors with path requests
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum PathError {
    /// No path was found
    NoPath,

    /// Invalid start node
    InvalidStartNode,

    /// Invalid end node
    InvalidEndNode,

    /// Invalid start time
    InvalidStartTime,

    /// Invalid end time
    InvalidEndTime,

    /// Invalid time window
    InvalidTimeWindow,

    /// Could not get client
    Client,

    /// Unknown error
    Unknown,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PathError::NoPath => write!(f, "No path was found."),
            PathError::InvalidStartNode => write!(f, "Invalid start node."),
            PathError::InvalidEndNode => write!(f, "Invalid end node."),
            PathError::InvalidStartTime => write!(f, "Invalid start time."),
            PathError::InvalidEndTime => write!(f, "Invalid end time."),
            PathError::InvalidTimeWindow => write!(f, "Invalid time window."),
            PathError::Client => write!(f, "Could not get client."),
            PathError::Unknown => write!(f, "Unknown error."),
        }
    }
}

#[derive(Debug)]
struct PathRequest {
    node_uuid_start: Uuid,
    node_uuid_end: Uuid,
    time_start: DateTime<Utc>,
    time_end: DateTime<Utc>,
}

/// Sanitize the request inputs
fn sanitize(request: BestPathRequest) -> Result<PathRequest, PathError> {
    let node_uuid_start = match uuid::Uuid::parse_str(&request.node_uuid_start) {
        Ok(uuid) => uuid,
        Err(_) => return Err(PathError::InvalidStartNode),
    };

    let node_uuid_end = match uuid::Uuid::parse_str(&request.node_uuid_end) {
        Ok(uuid) => uuid,
        Err(_) => return Err(PathError::InvalidEndNode),
    };

    let time_start = match request.time_start {
        None => chrono::Utc::now(),
        Some(time) => match timestamp_to_datetime(&time) {
            Some(time) => time,
            None => return Err(PathError::InvalidStartTime),
        },
    };

    let time_end = match request.time_end {
        None => chrono::Utc::now() + chrono::Duration::days(1),
        Some(time) => match timestamp_to_datetime(&time) {
            Some(time) => time,
            None => return Err(PathError::InvalidEndTime),
        },
    };

    if time_end < time_start {
        return Err(PathError::InvalidTimeWindow);
    }

    if time_end < Utc::now() {
        return Err(PathError::InvalidEndTime);
    }

    Ok(PathRequest {
        node_uuid_start,
        node_uuid_end,
        time_start,
        time_end,
    })
}

/// The purpose of this initial search is to verify that a flight between two
///  vertiports is physically possible.
///
/// A flight is physically impossible if the two vertiports cannot be
///  connected by a series of lines such that the aircraft never runs out
///  of charge.
///
/// No-Fly zones can extend flights, isolate aircraft, or disable vertiports entirely.
#[cfg(not(tarpaulin_include))]
pub async fn best_path(
    path_type: PathType,
    request: BestPathRequest,
    pool: deadpool_postgres::Pool,
) -> Result<Vec<PathSegment>, PathError> {
    let request = sanitize(request)?;

    let fn_name = match path_type {
        PathType::PortToPort => "best_path_p2p",
        PathType::AircraftToPort => "best_path_a2p",
    };

    let cmd_str = format!(
        "SELECT * FROM arrow.{fn_name}(
            '{}'::UUID,
            '{}'::UUID,
            '{}'::TIMESTAMPTZ,
            '{}'::TIMESTAMPTZ
        );",
        request.node_uuid_start, request.node_uuid_end, request.time_start, request.time_end
    );

    let client = match pool.get().await {
        Ok(client) => client,
        Err(e) => {
            println!("(get_paths) could not get client from pool.");
            println!("(get_paths) error: {:?}", e);
            return Err(PathError::Client);
        }
    };

    let rows = match client.query(&cmd_str, &[]).await {
        Ok(results) => results,
        Err(e) => {
            println!("(get_paths) could not request routes: {}", e);
            return Err(PathError::Unknown);
        }
    };

    let mut results: Vec<PathSegment> = vec![];
    for r in &rows {
        let start_type: super::NodeType = r.get(1);
        let start_latitude: f64 = r.get(2);
        let start_longitude: f64 = r.get(3);
        let end_type: super::NodeType = r.get(4);
        let end_latitude: f64 = r.get(5);
        let end_longitude: f64 = r.get(6);
        let distance_meters: f64 = r.get(7);

        let start_type = Into::<crate::grpc::server::NodeType>::into(start_type) as i32;
        let end_type = Into::<crate::grpc::server::NodeType>::into(end_type) as i32;

        results.push(PathSegment {
            index: r.get(0),
            start_type,
            start_latitude: start_latitude as f32,
            start_longitude: start_longitude as f32,
            end_type,
            end_latitude: end_latitude as f32,
            end_longitude: end_longitude as f32,
            distance_meters: distance_meters as f32,
            altitude_meters: ALTITUDE_HARDCODE, // TODO(R4): Corridors
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::server::grpc_server;
    use chrono::{Duration, Utc};
    use lib_common::time::datetime_to_timestamp;

    #[test]
    fn ut_sanitize_valid() {
        let request = BestPathRequest {
            node_uuid_start: uuid::Uuid::new_v4().to_string(),
            node_uuid_end: uuid::Uuid::new_v4().to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: None,
            time_end: None,
        };

        let result = sanitize(request);
        assert!(result.is_ok());
    }

    #[test]
    fn ut_sanitize_invalid_uuids() {
        let request = BestPathRequest {
            node_uuid_start: "Invalid".to_string(),
            node_uuid_end: uuid::Uuid::new_v4().to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: None,
            time_end: None,
        };

        let result = sanitize(request).unwrap_err();
        assert_eq!(result, PathError::InvalidStartNode);

        let request = BestPathRequest {
            node_uuid_start: uuid::Uuid::new_v4().to_string(),
            node_uuid_end: "Invalid".to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: None,
            time_end: None,
        };

        let result = sanitize(request).unwrap_err();
        assert_eq!(result, PathError::InvalidEndNode);
    }

    #[test]
    fn ut_sanitize_invalid_time_window() {
        let Some(time_start) = datetime_to_timestamp(&Utc::now()) else {
            panic!("(ut_sanitize_time) could not convert time to timestamp.");
        };

        let Some(time_end) = datetime_to_timestamp(&(Utc::now() - Duration::seconds(1))) else {
            panic!("(ut_sanitize_time) could not convert time to timestamp.");
        };

        // Start time is after end time
        let request = BestPathRequest {
            node_uuid_start: uuid::Uuid::new_v4().to_string(),
            node_uuid_end: uuid::Uuid::new_v4().to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: Some(time_start),
            time_end: Some(time_end.clone()),
        };

        let result = sanitize(request).unwrap_err();
        assert_eq!(result, PathError::InvalidTimeWindow);

        // Start time (assumed) is after current time
        let request = BestPathRequest {
            node_uuid_start: uuid::Uuid::new_v4().to_string(),
            node_uuid_end: uuid::Uuid::new_v4().to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: None,
            time_end: Some(time_end),
        };

        let result = sanitize(request).unwrap_err();
        assert_eq!(result, PathError::InvalidTimeWindow);

        // End time (assumed) is before start time
        let Some(time_start) = datetime_to_timestamp(&(Utc::now() + Duration::days(10))) else {
            panic!("(ut_sanitize_time) could not convert time to timestamp.");
        };
        let request = BestPathRequest {
            node_uuid_start: uuid::Uuid::new_v4().to_string(),
            node_uuid_end: uuid::Uuid::new_v4().to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: Some(time_start),
            time_end: None,
        };

        let result = sanitize(request).unwrap_err();
        assert_eq!(result, PathError::InvalidTimeWindow);
    }

    #[test]
    fn ut_sanitize_invalid_time_end() {
        // End time (assumed) is before start time
        let Some(time_start) = datetime_to_timestamp(&(Utc::now() - Duration::days(10))) else {
            panic!("(ut_sanitize_time) could not convert time to timestamp.");
        };

        let Some(time_end) = datetime_to_timestamp(&(Utc::now() - Duration::seconds(1))) else {
            panic!("(ut_sanitize_time) could not convert time to timestamp.");
        };

        // Won't route for a time in the past
        let request = BestPathRequest {
            node_uuid_start: uuid::Uuid::new_v4().to_string(),
            node_uuid_end: uuid::Uuid::new_v4().to_string(),
            start_type: grpc_server::NodeType::Vertiport as i32,
            time_start: Some(time_start),
            time_end: Some(time_end),
        };

        let result = sanitize(request).unwrap_err();
        assert_eq!(result, PathError::InvalidEndTime);
    }
}
