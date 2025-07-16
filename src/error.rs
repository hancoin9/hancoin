use thiserror::Error;
use warp::Rejection;

#[derive(Error, Debug)]
pub enum HancoinError {
    #[error("Missing field: {0}")]
    MissingField(String),
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    #[error("Account not found")]
    AccountNotFound,
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("System time error")]
    SystemTimeError,
    #[error("Faucet cooldown period not over")]
    FaucetCooldownNotOver,
    #[error("Total supply limit reached")]
    TotalSupplyLimitReached,
    #[error("Invalid transaction")]
    InvalidTransaction,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Internal server error")]
    InternalServerError,
}

impl warp::reject::Reject for HancoinError {}

pub fn handle_rejection(err: Rejection) -> Result<impl warp::Reply, std::convert::Infallible> {
    let code;
    let message;

    if let Some(e) = err.find::<HancoinError>() {
        match e {
            HancoinError::MissingField(_) | HancoinError::InvalidFormat(_) | HancoinError::InvalidTransaction | HancoinError::InvalidSignature => {
                code = warp::http::StatusCode::BAD_REQUEST;
            }
            HancoinError::AccountNotFound | HancoinError::SessionNotFound(_) => {
                code = warp::http::StatusCode::NOT_FOUND;
            }
            HancoinError::RateLimitExceeded => {
                code = warp::http::StatusCode::TOO_MANY_REQUESTS;
            }
            HancoinError::SystemTimeError | HancoinError::FaucetCooldownNotOver | HancoinError::TotalSupplyLimitReached | HancoinError::InternalServerError => {
                code = warp::http::StatusCode::INTERNAL_SERVER_ERROR;
            }
        }
        message = e.to_string();
    } else if err.is_not_found() {
        code = warp::http::StatusCode::NOT_FOUND;
        message = "Not Found";
    } else if let Some(_) = err.find::<warp::filters::body::BodyDeserializeError>() {
        code = warp::http::StatusCode::BAD_REQUEST;
        message = "Invalid JSON data";
    } else if let Some(_) = err.find::<warp::reject::MethodNotAllowed>() {
        code = warp::http::StatusCode::METHOD_NOT_ALLOWED;
        message = "Method not allowed";
    } else {
        code = warp::http::StatusCode::INTERNAL_SERVER_ERROR;
        message = "Internal Server Error";
    }

    let json = warp::reply::json(&serde_json::json!({
        "status": "error",
        "message": message
    }));

    Ok(warp::reply::with_status(json, code))
}
