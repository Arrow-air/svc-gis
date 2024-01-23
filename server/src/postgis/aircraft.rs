//! This module contains functions for updating aircraft in the PostGIS database.

use super::psql_transaction;
use super::PostgisError;
use crate::grpc::server::grpc_server;
use crate::postgis::utils::StringError;
use chrono::{DateTime, Utc};
use grpc_server::AircraftId as ReqAircraftId;
use grpc_server::AircraftPosition as ReqAircraftPos;
use grpc_server::AircraftType;
use grpc_server::AircraftVelocity as ReqAircraftVelocity;
use num_traits::FromPrimitive;
use postgis::ewkb::PointZ;

/// Allowed characters in a identifier
pub const IDENTIFIER_REGEX: &str = r"^[\-0-9A-Za-z_\.]{1,255}$";

/// Possible errors with aircraft requests
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum AircraftError {
    /// No aircraft were provided
    NoAircraft,

    /// Invalid Aircraft ID
    AircraftId,

    /// Invalid Location
    Location,

    /// Invalid Time Provided
    Time,

    /// Invalid Label
    Label,

    /// Could not get client
    Client,

    /// DBError error
    DBError,
}

impl std::fmt::Display for AircraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AircraftError::NoAircraft => write!(f, "No aircraft were provided."),
            AircraftError::AircraftId => write!(f, "Invalid aircraft ID provided."),
            AircraftError::Location => write!(f, "Invalid location provided."),
            AircraftError::Time => write!(f, "Invalid time provided."),
            AircraftError::Label => write!(f, "Invalid label provided."),
            AircraftError::Client => write!(f, "Could not get backend client."),
            AircraftError::DBError => write!(f, "Unknown backend error."),
        }
    }
}

struct AircraftPosition {
    identifier: String,
    geom: PointZ,
    timestamp: DateTime<Utc>,
}

struct AircraftId {
    identifier: String,
    aircraft_type: AircraftType,
    timestamp: DateTime<Utc>,
}

struct AircraftVelocity {
    identifier: String,
    velocity_horizontal_ground_mps: f32,
    velocity_vertical_mps: f32,
    track_angle_degrees: f32,
    timestamp: DateTime<Utc>,
}

/// Verifies that a identifier is valid
pub fn check_identifier(identifier: &str) -> Result<(), StringError> {
    super::utils::check_string(identifier, IDENTIFIER_REGEX)
}

/// Initializes the PostGIS database for aircraft.
pub async fn psql_init(pool: &deadpool_postgres::Pool) -> Result<(), PostgisError> {
    // Create Aircraft Table
    let table_name = "arrow.aircraft";
    let enum_name = "aircrafttype";
    let statements = vec![
        super::psql_enum_declaration::<AircraftType>(enum_name),
        format!(
            "CREATE TABLE IF NOT EXISTS {table_name} (
            identifier VARCHAR(20) UNIQUE PRIMARY KEY NOT NULL,
            aircraft_type {enum_name} NOT NULL DEFAULT '{}',
            velocity_horizontal_ground_mps FLOAT(4),
            velocity_vertical_mps FLOAT(4),
            track_angle_degrees FLOAT(4),
            geom GEOMETRY(POINTZ, 4326),
            last_identifier_update TIMESTAMPTZ,
            last_position_update TIMESTAMPTZ,
            last_velocity_update TIMESTAMPTZ
        );",
            AircraftType::Undeclared.to_string()
        ),
    ];

    psql_transaction(statements, pool).await
}

impl TryFrom<ReqAircraftPos> for AircraftPosition {
    type Error = PostgisError;

    fn try_from(craft: ReqAircraftPos) -> Result<Self, Self::Error> {
        if let Err(e) = check_identifier(&craft.identifier) {
            postgis_error!(
                "(try_from ReqAircraftPos) Invalid aircraft identifier: {}; {}",
                craft.identifier,
                e
            );
            return Err(PostgisError::Aircraft(AircraftError::Label));
        }

        let Some(geom) = craft.geom else {
            postgis_error!("(try_from ReqAircraftPos) Aircraft location is invalid.");
            return Err(PostgisError::Aircraft(AircraftError::Location));
        };

        let geom = PointZ {
            x: geom.longitude,
            y: geom.latitude,
            z: geom.altitude_meters as f64,
            srid: Some(4326),
        };

        if let Err(e) = super::utils::validate_pointz(&geom) {
            postgis_error!(
                "(try_from ReqAircraftPos) Invalid aircraft location: {}; {}",
                craft.identifier,
                e
            );
            return Err(PostgisError::Aircraft(AircraftError::Location));
        }

        let Some(timestamp) = craft.timestamp_network else {
            postgis_error!("(try_from ReqAircraftPos) Aircraft time is invalid.");
            return Err(PostgisError::Aircraft(AircraftError::Time));
        };

        Ok(AircraftPosition {
            identifier: craft.identifier,
            geom,
            timestamp: timestamp.into(),
        })
    }
}

impl TryFrom<ReqAircraftId> for AircraftId {
    type Error = PostgisError;

    fn try_from(craft: ReqAircraftId) -> Result<Self, Self::Error> {
        if let Err(e) = check_identifier(&craft.identifier) {
            postgis_error!(
                "(try_from ReqAircraftId) Invalid aircraft identifier: {}; {}",
                craft.identifier,
                e
            );
            return Err(PostgisError::Aircraft(AircraftError::Label));
        }

        let Some(aircraft_type) = FromPrimitive::from_i32(craft.aircraft_type) else {
            postgis_error!(
                "(try_from ReqAircraftId) Invalid aircraft type: {}",
                craft.aircraft_type
            );

            return Err(PostgisError::Aircraft(AircraftError::AircraftId));
        };

        let Some(timestamp) = craft.timestamp_network else {
            postgis_error!("(try_from ReqAircraftPos) Aircraft time is invalid.");
            return Err(PostgisError::Aircraft(AircraftError::Time));
        };

        Ok(AircraftId {
            identifier: craft.identifier,
            aircraft_type,
            timestamp: timestamp.into(),
        })
    }
}

impl TryFrom<ReqAircraftVelocity> for AircraftVelocity {
    type Error = PostgisError;

    fn try_from(craft: ReqAircraftVelocity) -> Result<Self, Self::Error> {
        if let Err(e) = check_identifier(&craft.identifier) {
            postgis_error!(
                "(try_from ReqAircraftVelocity) Invalid aircraft identifier: {}; {}",
                craft.identifier,
                e
            );
            return Err(PostgisError::Aircraft(AircraftError::Label));
        }

        let Some(timestamp) = craft.timestamp_network else {
            postgis_error!("(try_from ReqAircraftVelocity) Network time is invalid.");
            return Err(PostgisError::Aircraft(AircraftError::Time));
        };

        Ok(AircraftVelocity {
            identifier: craft.identifier,
            velocity_horizontal_ground_mps: craft.velocity_horizontal_ground_mps,
            velocity_vertical_mps: craft.velocity_vertical_mps,
            track_angle_degrees: craft.track_angle_degrees,
            timestamp: timestamp.into(),
        })
    }
}

/// Updates aircraft in the PostGIS database.
pub async fn update_aircraft_id(
    aircraft: Vec<ReqAircraftId>,
    pool: &deadpool_postgres::Pool,
) -> Result<(), PostgisError> {
    postgis_debug!("(update_aircraft_id) entry.");
    if aircraft.is_empty() {
        return Err(PostgisError::Aircraft(AircraftError::NoAircraft));
    }

    let aircraft: Vec<AircraftId> = aircraft
        .into_iter()
        .map(AircraftId::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    let mut client = pool.get().await.map_err(|e| {
        postgis_error!(
            "(update_aircraft_id) could not get client from psql connection pool: {}",
            e
        );
        PostgisError::Aircraft(AircraftError::Client)
    })?;
    let transaction = client.transaction().await.map_err(|e| {
        postgis_error!("(update_aircraft_id) could not create transaction: {}", e);
        PostgisError::Aircraft(AircraftError::DBError)
    })?;

    let stmt = transaction
        .prepare_cached(
            "
        INSERT INTO arrow.aircraft(identifier, aircraft_type, last_identifier_update)
        VALUES ($1, $2, $3)
        ON CONFLICT (identifier) DO UPDATE
            SET aircraft_type = $2,
                last_identifier_update = $3;
        ",
        )
        .await
        .map_err(|e| {
            postgis_error!(
                "(update_aircraft_id) could not prepare cached statement: {}",
                e
            );
            PostgisError::Aircraft(AircraftError::DBError)
        })?;

    for craft in &aircraft {
        transaction
            .execute(
                &stmt,
                &[&craft.identifier, &craft.aircraft_type, &craft.timestamp],
            )
            .await
            .map_err(|e| {
                postgis_error!("(update_aircraft_id) could not execute transaction: {}", e);
                PostgisError::Aircraft(AircraftError::DBError)
            })?;
    }

    match transaction.commit().await {
        Ok(_) => {
            postgis_debug!("(update_aircraft_id) success.");
            Ok(())
        }
        Err(e) => {
            postgis_error!("(update_aircraft_id) could not commit transaction: {}", e);
            Err(PostgisError::Aircraft(AircraftError::DBError))
        }
    }
}

/// Updates aircraft position in the PostGIS database.
pub async fn update_aircraft_position(
    aircraft: Vec<ReqAircraftPos>,
    pool: &deadpool_postgres::Pool,
) -> Result<(), PostgisError> {
    postgis_debug!("(update_aircraft_position) entry.");
    if aircraft.is_empty() {
        return Err(PostgisError::Aircraft(AircraftError::NoAircraft));
    }

    let aircraft: Vec<AircraftPosition> = aircraft
        .into_iter()
        .map(AircraftPosition::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    let mut client = pool.get().await.map_err(|e| {
        postgis_error!(
            "(update_aircraft_position) could not get client from psql connection pool: {}",
            e
        );
        PostgisError::Aircraft(AircraftError::Client)
    })?;

    let transaction = client.transaction().await.map_err(|e| {
        postgis_error!(
            "(update_aircraft_position) could not create transaction: {}",
            e
        );
        PostgisError::Aircraft(AircraftError::DBError)
    })?;

    let stmt = transaction
        .prepare_cached(
            "
        INSERT INTO arrow.aircraft (identifier, geom, last_position_update)
        VALUES ($1, $2, $3)
        ON CONFLICT (identifier) DO UPDATE
            SET geom = $2,
                last_position_update = $3;
        ",
        )
        .await
        .map_err(|e| {
            postgis_error!(
                "(update_aircraft_position) could not prepare cached statement: {}",
                e
            );
            PostgisError::Aircraft(AircraftError::DBError)
        })?;

    for craft in &aircraft {
        transaction
            .execute(&stmt, &[&craft.identifier, &craft.geom, &craft.timestamp])
            .await
            .map_err(|e| {
                postgis_error!(
                    "(update_aircraft_position) could not execute transaction: {}",
                    e
                );
                PostgisError::Aircraft(AircraftError::DBError)
            })?;
    }

    match transaction.commit().await {
        Ok(_) => {
            postgis_debug!("(update_aircraft_position) success.");
            Ok(())
        }
        Err(e) => {
            postgis_error!(
                "(update_aircraft_position) could not commit transaction: {}",
                e
            );
            Err(PostgisError::Aircraft(AircraftError::DBError))
        }
    }
}

/// Updates aircraft velocity in the PostGIS database.
pub async fn update_aircraft_velocity(
    aircraft: Vec<ReqAircraftVelocity>,
    pool: &deadpool_postgres::Pool,
) -> Result<(), PostgisError> {
    postgis_debug!("(update_aircraft_position) entry.");
    if aircraft.is_empty() {
        return Err(PostgisError::Aircraft(AircraftError::NoAircraft));
    }

    let aircraft: Vec<AircraftVelocity> = aircraft
        .into_iter()
        .map(AircraftVelocity::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    let mut client = pool.get().await.map_err(|e| {
        postgis_error!(
            "(update_aircraft_velocity) could not get client from psql connection pool: {}",
            e
        );
        PostgisError::Aircraft(AircraftError::Client)
    })?;
    let transaction = client.transaction().await.map_err(|e| {
        postgis_error!(
            "(update_aircraft_velocity) could not create transaction: {}",
            e
        );
        PostgisError::Aircraft(AircraftError::DBError)
    })?;

    let stmt = transaction
        .prepare_cached(
            "
        INSERT INTO arrow.aircraft (
            identifier,
            velocity_horizontal_ground_mps,
            velocity_vertical_mps,
            track_angle_degrees,
            last_velocity_update
        ) VALUES (
            $1, $2, $3, $4, $5
        ) ON CONFLICT (identifier) DO UPDATE
            SET velocity_horizontal_ground_mps = $2,
                velocity_vertical_mps = $3,
                track_angle_degrees = $4,
                last_velocity_update = $5;",
        )
        .await
        .map_err(|e| {
            postgis_error!(
                "(update_aircraft_velocity) could not prepare cached statement: {}",
                e
            );
            PostgisError::Aircraft(AircraftError::DBError)
        })?;

    for craft in &aircraft {
        transaction
            .execute(
                &stmt,
                &[
                    &craft.identifier,
                    &craft.velocity_horizontal_ground_mps,
                    &craft.velocity_vertical_mps,
                    &craft.track_angle_degrees,
                    &craft.timestamp,
                ],
            )
            .await
            .map_err(|e| {
                postgis_error!(
                    "(update_aircraft_velocity) could not execute transaction: {}",
                    e
                );
                PostgisError::Aircraft(AircraftError::DBError)
            })?;
    }

    match transaction.commit().await {
        Ok(_) => {
            postgis_debug!("(update_aircraft_velocity) success.");
            Ok(())
        }
        Err(e) => {
            postgis_error!(
                "(update_aircraft_velocity) could not commit transaction: {}",
                e
            );
            Err(PostgisError::Aircraft(AircraftError::DBError))
        }
    }
}

/// Gets the geometry of an aircraft given its identifier.
pub async fn get_aircraft_pointz(
    identifier: &str,
    pool: &deadpool_postgres::Pool,
) -> Result<PointZ, PostgisError> {
    let stmt = "SELECT geom FROM arrow.aircraft WHERE identifier = $1;";
    let client = pool.get().await.map_err(|e| {
        postgis_error!(
            "(get_aircraft_pointz) could not get client from psql connection pool: {}",
            e
        );
        PostgisError::Aircraft(AircraftError::Client)
    })?;

    client
        .query_one(stmt, &[&identifier])
        .await
        .map_err(|e| {
            postgis_error!("(get_aircraft_pointz) could not prepare cached statement: {}", e);
            PostgisError::Aircraft(AircraftError::DBError)
        })?
        .try_get::<_, PointZ>(0)
        .map_err(|e| {
            postgis_error!("(get_aircraft_pointz) zero or more than one records found for aircraft '{identifier}': {}", e);
            PostgisError::Aircraft(AircraftError::DBError)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::get_psql_pool;
    use grpc_server::PointZ as GrpcPointZ;
    use lib_common::time::*;

    #[tokio::test]
    async fn ut_request_valid() {
        crate::get_log_handle().await;
        ut_info!("(ut_request_valid) start");

        let nodes: Vec<(&str, f64, f64, f32)> = vec![
            ("Marauder", 52.3745905, 4.9160036, 100.0),
            ("Phantom", 52.3749819, 4.9156925, 102.0),
            ("Ghost", 52.3752144, 4.9153733, 45.0),
            ("Falcon", 52.3753012, 4.9156845, 50.),
            ("Mantis", 52.3750703, 4.9161538, 72.),
        ];

        let aircraft: Vec<ReqAircraftPos> = nodes
            .iter()
            .map(|(label, latitude, longitude, altitude)| ReqAircraftPos {
                identifier: label.to_string(),
                geom: Some(GrpcPointZ {
                    latitude: *latitude,
                    longitude: *longitude,
                    altitude_meters: *altitude,
                }),
                timestamp_network: Some(Utc::now().into()),
                timestamp_aircraft: None,
            })
            .collect();

        let converted = aircraft
            .clone()
            .into_iter()
            .map(AircraftPosition::try_from)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(aircraft.len(), converted.len());

        for (i, aircraft) in aircraft.iter().enumerate() {
            assert_eq!(aircraft.identifier, converted[i].identifier);
            let geom = aircraft.geom.unwrap();
            assert_eq!(geom.latitude, converted[i].geom.y);
            assert_eq!(geom.longitude, converted[i].geom.x);
            assert_eq!(geom.altitude_meters as f64, converted[i].geom.z);

            let time: Timestamp = aircraft
                .timestamp_network
                .clone()
                .expect("Expected Some time.");
            let converted: Timestamp = converted[i].timestamp.into();

            assert_eq!(time, converted);
        }

        ut_info!("(ut_request_valid) success");
    }

    #[tokio::test]
    async fn ut_client_failure() {
        crate::get_log_handle().await;
        ut_info!("(ut_client_failure) start");

        let nodes = vec![("aircraft", 52.3745905, 4.9160036)];
        let aircraft: Vec<ReqAircraftPos> = nodes
            .iter()
            .map(|(label, latitude, longitude)| ReqAircraftPos {
                identifier: label.to_string(),
                geom: Some(GrpcPointZ {
                    latitude: *latitude,
                    longitude: *longitude,
                    altitude_meters: 100.0,
                }),
                timestamp_network: Some(Utc::now().into()),
                ..Default::default()
            })
            .collect();

        let result = update_aircraft_position(aircraft, get_psql_pool().await)
            .await
            .unwrap_err();
        assert_eq!(result, PostgisError::Aircraft(AircraftError::Client));

        ut_info!("(ut_client_failure) success");
    }

    #[tokio::test]
    async fn ut_aircraft_request_to_gis_invalid_label() {
        crate::get_log_handle().await;
        ut_info!("(ut_aircraft_request_to_gis_invalid_label) start");

        for label in &[
            "NULL",
            "Aircraft;",
            "'Aircraft'",
            "Aircraft \'",
            &"X".repeat(1000),
        ] {
            let aircraft: Vec<ReqAircraftPos> = vec![ReqAircraftPos {
                identifier: label.to_string(),
                ..Default::default()
            }];

            let result = update_aircraft_position(aircraft, get_psql_pool().await)
                .await
                .unwrap_err();
            assert_eq!(result, PostgisError::Aircraft(AircraftError::Label));
        }

        ut_info!("(ut_aircraft_request_to_gis_invalid_label) success");
    }

    #[tokio::test]
    async fn ut_aircraft_request_to_gis_invalid_no_nodes() {
        crate::get_log_handle().await;
        ut_info!("(ut_aircraft_request_to_gis_invalid_no_nodes) start");

        let aircraft: Vec<ReqAircraftPos> = vec![];
        let result = update_aircraft_position(aircraft, get_psql_pool().await)
            .await
            .unwrap_err();
        assert_eq!(result, PostgisError::Aircraft(AircraftError::NoAircraft));

        ut_info!("(ut_aircraft_request_to_gis_invalid_no_nodes) success");
    }

    #[tokio::test]
    async fn ut_aircraft_request_to_gis_invalid_location() {
        crate::get_log_handle().await;
        ut_info!("(ut_aircraft_request_to_gis_invalid_location) start");

        let coords = vec![(-90.1, 0.0), (90.1, 0.0), (0.0, -180.1), (0.0, 180.1)];
        for coord in coords {
            let aircraft: Vec<ReqAircraftPos> = vec![ReqAircraftPos {
                geom: Some(GrpcPointZ {
                    latitude: coord.0,
                    longitude: coord.1,
                    altitude_meters: 100.0,
                }),
                identifier: "Aircraft".to_string(),
                timestamp_network: Some(Utc::now().into()),
                ..Default::default()
            }];

            let result = update_aircraft_position(aircraft, get_psql_pool().await)
                .await
                .unwrap_err();
            assert_eq!(result, PostgisError::Aircraft(AircraftError::Location));
        }

        // No location
        let aircraft: Vec<ReqAircraftPos> = vec![ReqAircraftPos {
            geom: None,
            identifier: "Aircraft".to_string(),
            ..Default::default()
        }];

        let result = update_aircraft_position(aircraft, get_psql_pool().await)
            .await
            .unwrap_err();
        assert_eq!(result, PostgisError::Aircraft(AircraftError::Location));

        ut_info!("(ut_aircraft_request_to_gis_invalid_location) success");
    }

    #[tokio::test]
    async fn ut_aircraft_request_to_gis_invalid_time() {
        crate::get_log_handle().await;
        ut_info!("(ut_aircraft_request_to_gis_invalid_time) start");

        // No location
        let aircraft: Vec<ReqAircraftPos> = vec![ReqAircraftPos {
            timestamp_network: None,
            geom: Some(GrpcPointZ {
                latitude: 0.0,
                longitude: 0.0,
                altitude_meters: 0.0,
            }),
            identifier: "Aircraft".to_string(),
            ..Default::default()
        }];

        let result = update_aircraft_position(aircraft, get_psql_pool().await)
            .await
            .unwrap_err();
        assert_eq!(result, PostgisError::Aircraft(AircraftError::Time));

        ut_info!("(ut_aircraft_request_to_gis_invalid_time) success");
    }
}
