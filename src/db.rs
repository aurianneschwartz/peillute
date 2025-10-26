//! Database management for the Peillute application
//!
//! This module handles all database operations, including user management,
//! transaction processing, and state synchronization. It uses SQLite as the
//! underlying database engine.

/// Represents a transaction in the system
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    /// Source user of the transaction
    pub from_user: String,
    /// Destination user of the transaction
    pub to_user: String,
    /// Transaction amount
    pub amount: f64,
    /// Lamport timestamp of the transaction
    pub lamport_time: i64,
    /// ID of the node that created the transaction
    pub source_node: String,
    /// Optional message associated with the transaction
    pub optional_msg: Option<String>,
    /// Vector clock state at the time of the transaction
    pub vector_clock: std::collections::HashMap<String, i64>,
}

#[allow(unused_imports)]
use clap::Parser;
#[cfg(feature = "server")]
lazy_static::lazy_static! {
    pub static ref DB_CONN: std::sync::Mutex<rusqlite::Connection> =
        std::sync::Mutex::new(rusqlite::Connection::open(format!("peillute_{}.db", super::Args::parse().cli_db_id)).unwrap());
}

#[cfg(feature = "server")]
/// Special value representing a null user
const NULL: &str = "NULL";

#[cfg(feature = "server")]
/// Initializes the database schema
pub fn init_db() -> rusqlite::Result<()> {
    init_db_with_metrics(None)
}

#[cfg(feature = "server")]
pub fn init_db_with_metrics(
    metrics: Option<&opentelemetry::metrics::Counter<u64>>,
) -> rusqlite::Result<()> {
    {
        let conn = DB_CONN.lock().unwrap();

        // Create VectorClock table for storing vector clock states
        conn.execute(
            "CREATE TABLE IF NOT EXISTS VectorClock (
            id INTEGER PRIMARY KEY AUTOINCREMENT
        );",
            [],
        )?;

        // Create VectorClockEntry table for storing individual vector clock entries
        conn.execute(
            "CREATE TABLE IF NOT EXISTS VectorClockEntry (
                vector_clock_id INTEGER,
                site_id TEXT,
                value INTEGER NOT NULL,
                PRIMARY KEY(vector_clock_id, site_id),
                FOREIGN KEY(vector_clock_id) REFERENCES VectorClock(id) ON DELETE CASCADE
            );
            ",
            [],
        )?;

        // Create User table for storing user accounts
        conn.execute(
            "CREATE TABLE IF NOT EXISTS User (
            unique_name TEXT PRIMARY KEY,
            solde FLOAT NOT NULL
        )",
            [],
        )?;

        // Create Transactions table for storing transaction history
        conn.execute(
            "CREATE TABLE IF NOT EXISTS Transactions (
                from_user TEXT,
                to_user TEXT NOT NULL,
                amount FLOAT NOT NULL,
                lamport_time INTEGER NOT NULL,
                vector_clock_id INTEGER NOT NULL,
                source_node TEXT NOT NULL,
                optional_msg TEXT,
                FOREIGN KEY(from_user) REFERENCES User(unique_name),
                FOREIGN KEY(to_user) REFERENCES User(unique_name),
                FOREIGN KEY(vector_clock_id) REFERENCES VectorClock(id),
                PRIMARY KEY(lamport_time, source_node)
            );",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS LocalState (
            site_id TEXT PRIMARY KEY,
            lamport_time INTEGER NOT NULL,
            vector_clock_id INTEGER NOT NULL,
            FOREIGN KEY(vector_clock_id) REFERENCES VectorClock(id)
        );",
            [],
        )?;
    }

    if let Some(counter) = metrics {
        counter.add(1, &[opentelemetry::KeyValue::new("component", "database")]);
    }

    tracing::debug!(operation = "init_db", "Database initialized successfully.");
    Ok(())
}

#[cfg(feature = "server")]
/// Update the local state of the site
pub fn update_local_state(site_id: &str, clock: crate::clock::Clock) -> rusqlite::Result<()> {
    use rusqlite::params;

    let lamport_time = clock.get_lamport();
    let vc_clock = clock.get_vector_clock_map();

    let conn = DB_CONN.lock().unwrap();
    conn.execute("INSERT INTO VectorClock DEFAULT VALUES", [])?;
    let vector_clock_id = conn.last_insert_rowid();

    let mut stmt = conn.prepare(
        "INSERT INTO VectorClockEntry (vector_clock_id, site_id, value) VALUES (?1, ?2, ?3)",
    )?;
    for (site, value) in vc_clock.iter() {
        stmt.execute(params![vector_clock_id, site, value])?;
    }
    conn.execute(
        "INSERT OR REPLACE INTO LocalState (site_id, lamport_time, vector_clock_id)
        VALUES (?1, ?2, ?3)",
        params![site_id, lamport_time, vector_clock_id],
    )?;
    tracing::info!(
        operation = "update_local_state",
        site = %site_id,
        "Updated the local state of the site."
    );
    Ok(())
}

#[cfg(feature = "server")]
/// Update the database with a snapshot
pub fn update_db_with_snapshot(
    snapshot: &crate::snapshot::GlobalSnapshot,
    vector_clock: &std::collections::HashMap<String, i64>,
) {
    tracing::info!(
        operation = "update_db_with_snapshot",
        "Applying snapshot to database"
    );

    if snapshot.missing.is_empty() {
        tracing::info!("No missing transactions, nothing to do");
        return;
    }

    // sort tsx actions by lamport time
    let mut sorted_txs: Vec<_> = snapshot
        .missing
        .values()
        .flat_map(|txs| txs.iter())
        .collect();
    sorted_txs.sort_by_key(|tx| tx.lamport_time);

    for tx in sorted_txs {
        let optional_msg = "";

        let _ = crate::db::create_transaction(
            &tx.from_user,
            &tx.to_user,
            (tx.amount_in_cent as f64) / 100.0,
            &tx.lamport_time,
            &tx.source_node,
            &optional_msg,
            vector_clock,
        );
    }
}

#[cfg(feature = "server")]
/// Get the local state of the site
pub fn get_local_state() -> rusqlite::Result<(String, crate::clock::Clock)> {
    use rusqlite::params;
    let conn = DB_CONN.lock().unwrap();
    let mut stmt =
        conn.prepare("SELECT site_id, lamport_time, vector_clock_id FROM LocalState LIMIT 1")?;

    let row = stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)));

    let (site_id, lamport_time, vector_clock_id): (String, i64, i64) = match row {
        Ok(data) => data,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Err(e) => return Err(e),
    };

    let mut clock_map = std::collections::HashMap::new();
    let mut vc_stmt =
        conn.prepare("SELECT site_id, value FROM VectorClockEntry WHERE vector_clock_id = ?1")?;
    let mut rows = vc_stmt.query(params![vector_clock_id])?;
    while let Some(vc_row) = rows.next()? {
        let site: String = vc_row.get(0)?;
        let value: i64 = vc_row.get(1)?;
        clock_map.insert(site, value);
    }

    let c = crate::clock::Clock::from_parts(lamport_time, clock_map);

    Ok((site_id, c))
}

#[cfg(feature = "server")]
/// Checks if the database has been initialized
pub fn is_database_initialized() -> rusqlite::Result<bool> {
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'Transactions')",
        )?;
        let exists: bool = stmt.query_row([], |row| row.get(0))?;
        Ok(exists)
    }
}

#[cfg(feature = "server")]
/// Check if a transaction exists in the database
pub fn transaction_exists(lamport_time: i64, source_node: &str) -> rusqlite::Result<bool> {
    use rusqlite::params;
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT EXISTS(SELECT 1 FROM Transactions WHERE lamport_time = ?1 AND source_node = ?2)",
        )?;
        let exists: bool = stmt.query_row(params![lamport_time, source_node], |row| row.get(0))?;
        Ok(exists)
    }
}

#[cfg(feature = "server")]
/// Checks if a user exists in the database
pub fn user_exists(name: &str) -> rusqlite::Result<bool> {
    {
        use rusqlite::params;
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare("SELECT EXISTS(SELECT 1 FROM User WHERE unique_name = ?1)")?;
        let exists: bool = stmt.query_row(params![name], |row| row.get(0))?;
        Ok(exists)
    }
}

#[cfg(feature = "server")]
/// Creates a new user with zero balance
pub fn create_user(unique_name: &str) -> rusqlite::Result<()> {
    let mut _guard = crate::metrics::OpGuardSimple::new("database", "create_user");
    use rusqlite::params;
    if user_exists(unique_name)? {
        tracing::warn!(
            operation = "create_user",
            "User '{}' already exists.",
            unique_name
        );
        return Ok(());
    }

    let result = {
        tracing::debug!(operation = "create_user", "Adding user {}", unique_name);
        let conn = DB_CONN.lock().unwrap();
        conn.execute(
            "INSERT INTO User (unique_name, solde) VALUES (?1, 0)",
            params![unique_name],
        )
    };

    match result {
        Ok(_) => {
            crate::metrics::PROM_DB_USER_CREATED.inc();
            Ok(())
        }
        Err(e) => {
            _guard.mark_error();
            Err(e)
        }
    }
}

#[cfg(feature = "server")]
/// Deletes a user from the database
pub fn delete_user(name: &str) -> rusqlite::Result<()> {
    use rusqlite::params;
    if !user_exists(name)? {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("User '{}' does not exist.", name).into()),
        );

        tracing::error!(
            operation = "delete_user",
            name = %name,
            "User does not exist."
        );
        return Err(err);
    }
    {
        let conn = DB_CONN.lock().unwrap();
        conn.execute("DELETE FROM User WHERE unique_name = ?1", params![name])?;
        Ok(())
    }
}

#[cfg(feature = "server")]
/// Calculates the current balance for a user
pub fn calculate_solde(name: &str) -> rusqlite::Result<f64> {
    {
        use rusqlite::params;
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT
            IFNULL((SELECT SUM(amount) FROM Transactions WHERE to_user = ?1), 0) -
            IFNULL((SELECT SUM(amount) FROM Transactions WHERE from_user = ?1), 0)
        AS balance",
        )?;
        stmt.query_row(params![name], |row| row.get(0))
    }
}

#[cfg(feature = "server")]
/// Updates the stored balance for a user
pub fn update_solde(name: &str) -> rusqlite::Result<()> {
    use rusqlite::params;

    if !user_exists(name)? {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("User '{}' does not exist.", name).into()),
        );

        return Err(err);
    }
    let solde = calculate_solde(name)?;
    {
        let conn = DB_CONN.lock().unwrap();
        conn.execute(
            "UPDATE User SET solde = ?1 WHERE unique_name = ?2",
            params![solde, name],
        )?;
        tracing::debug!(
            operation = "update_user_balance",
            "Updated balance for {} to {}",
            name,
            solde
        );
        Ok(())
    }
}

#[cfg(feature = "server")]
/// Ensures a user exists, creating it if necessary
pub fn ensure_user(name: &str) -> rusqlite::Result<()> {
    if name != NULL && !user_exists(name)? {
        create_user(name)?;
    }
    Ok(())
}

#[cfg(feature = "server")]
/// Creates a new transaction between users
pub fn create_transaction(
    from_user: &str,
    to_user: &str,
    amount: f64,
    lamport_time: &i64,
    source_node: &str,
    optional_msg: &str,
    vector_clock: &std::collections::HashMap<String, i64>,
) -> rusqlite::Result<()> {
    let mut _guard = crate::metrics::OpGuardSimple::new("database", "create_transaction");
    use rusqlite::params;
    if from_user != NULL && calculate_solde(from_user)? < amount {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(
                format!(
                    "Insufficient funds: '{}' has less than {}.",
                    from_user, amount
                )
                .into(),
            ),
        );

        tracing::error!(
            operation = "create_transaction",
            "Insufficient funds: '{}' has less than {}.",
            from_user,
            amount
        );
        _guard.mark_error();
        return Err(err);
    }

    ensure_user(from_user)?;
    ensure_user(to_user)?;

    tracing::debug!(
        operation = "create_transaction",
        "Creating transaction from {} to {} with amount {}",
        from_user,
        to_user,
        amount
    );

    {
        let conn = DB_CONN.lock().unwrap();
        conn.execute("INSERT INTO VectorClock DEFAULT VALUES", [])?;
        let vector_clock_id = conn.last_insert_rowid();

        let mut stmt = conn.prepare(
            "INSERT INTO VectorClockEntry (vector_clock_id, site_id, value) VALUES (?1, ?2, ?3)",
        )?;
        for (site_id, value) in vector_clock.iter() {
            stmt.execute(params![vector_clock_id, site_id, value])?;
        }

        conn.execute(
        "INSERT INTO Transactions (from_user, to_user, amount, lamport_time, vector_clock_id, source_node, optional_msg)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            from_user,
            to_user,
            amount,
            lamport_time,
            vector_clock_id,
            source_node,
            optional_msg
        ],
    )?;
    }

    if from_user != NULL {
        update_solde(from_user)?;
    }
    if to_user != NULL {
        update_solde(to_user)?;
    }

    crate::metrics::PROM_DB_TRANSACTION_CREATED.inc();

    Ok(())
}

#[cfg(feature = "server")]
pub fn deposit(
    user: &str,
    amount: f64,
    lamport_time: &i64,
    source_node: &str,
    vector_clock: &std::collections::HashMap<String, i64>,
) -> rusqlite::Result<()> {
    let _guard = crate::metrics::OpGuardSimple::new("database", "deposit");
    if !user_exists(user)? {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("Unknown User: {}", user).into()),
        );

        tracing::error!(
            operation = "deposit",
            user = %user,
            "Unknown User"
        );
        return Err(err);
    }

    if amount < 0.0 {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("Negative deposit amount: {}", amount).into()),
        );

        tracing::error!(
            operation = "deposit",
            amount = amount,
            "Negative deposit amount"
        );
        return Err(err);
    }

    tracing::debug!(operation = "deposit", "Depositing {} to {}", amount, user);

    create_transaction(
        NULL,
        user,
        amount,
        lamport_time,
        source_node,
        "Deposit",
        vector_clock,
    )
}

#[cfg(feature = "server")]
pub fn withdraw(
    user: &str,
    amount: f64,
    lamport_time: &i64,
    source_node: &str,
    vector_clock: &std::collections::HashMap<String, i64>,
) -> rusqlite::Result<()> {
    let _guard = crate::metrics::OpGuardSimple::new("database", "withdraw");
    if amount < 0.0 {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("Negative withdrawal amount: {}", amount).into()),
        );
        tracing::error!(
            operation = "withdraw",
            amount = amount,
            "Negative withdrawal amount"
        );
        return Err(err);
    }
    if !user_exists(user)? {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("Unknown user: {}", user).into()),
        );
        tracing::error!(
            operation = "withdraw",
            user = %user,
            "Unknown user"
        );
        return Err(err);
    }
    if calculate_solde(user)? < amount {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(format!("User {} not enough money", user).into()),
        );
        tracing::error!(
            operation = "withdraw",
            user = %user,
            "User has not enough money"
        );
        return Err(err);
    }

    tracing::debug!(
        operation = "withdraw",
        "Withdrawing {} from {}",
        amount,
        user
    );

    create_transaction(
        user,
        NULL,
        amount,
        lamport_time,
        source_node,
        "Withdraw",
        vector_clock,
    )
}

#[cfg(feature = "server")]
pub fn has_been_refunded(transac_time: i64, node: &str) -> rusqlite::Result<bool> {
    use rusqlite::params;
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT EXISTS(SELECT 1 FROM Transactions WHERE optional_msg = ?1)")?;

        let optional_msg = format!("Refund transaction {}-{}", node, transac_time);
        let exists: bool = stmt.query_row(params![optional_msg], |row| row.get(0))?;

        Ok(exists)
    }
}

#[cfg(feature = "server")]
pub fn refund_transaction(
    transac_time: i64,
    node: &str,
    lamport_time: &i64,
    source_node: &str,
    vector_clock: &std::collections::HashMap<String, i64>,
) -> rusqlite::Result<()> {
    if let Some(tx) = get_transaction(transac_time, node)? {
        if calculate_solde(&tx.to_user)? < tx.amount {
            let err = rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
                Some(format!("User {} has not enough money to give back", &tx.to_user).into()),
            );
            tracing::error!(
                operation = "refund_transaction",
                "User {} has not enough money to give back",
                &tx.to_user
            );
            return Err(err);
        }

        if tx.optional_msg.is_some() && tx.optional_msg.unwrap().starts_with("Refund transaction") {
            let err = rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
                Some(
                    format!(
                        "Transaction {}-{} is a refund transaction",
                        node, transac_time
                    )
                    .into(),
                ),
            );
            tracing::error!(
                operation = "refund_transaction",
                "Transaction {}-{} is a refund transaction",
                node,
                transac_time
            );
            return Err(err);
        }

        if has_been_refunded(transac_time, node)? {
            let err = rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
                Some(format!("Transaction {}-{} already refunded", node, transac_time).into()),
            );
            tracing::error!(
                operation = "refund_transaction",
                "Transaction {}-{} already refunded",
                node,
                transac_time
            );
            return Err(err);
        }

        create_transaction(
            &tx.to_user,
            &tx.from_user,
            tx.amount,
            lamport_time,
            source_node,
            &format!("Refund transaction {}-{}", node, transac_time),
            vector_clock,
        )?;
    } else {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::ErrorCode::Unknown as i32),
            Some(
                format!(
                    "No transaction found at time {} from node {}",
                    transac_time, node
                )
                .into(),
            ),
        );

        tracing::error!(
            "No transaction found at time {} from node {}",
            transac_time,
            node
        );
        return Err(err);
    }
    Ok(())
}

#[cfg(feature = "server")]
pub fn get_transaction(transac_time: i64, node: &str) -> rusqlite::Result<Option<Transaction>> {
    use rusqlite::params;
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT from_user, to_user, amount, lamport_time, source_node, optional_msg, vector_clock_id
        FROM Transactions WHERE lamport_time = ?1 AND source_node = ?2",
        )?;

        match stmt.query_row(params![transac_time, node], |row| {
            let from_user: String = row.get(0)?;
            let to_user: String = row.get(1)?;
            let amount: f64 = row.get(2)?;
            let lamport_time: i64 = row.get(3)?;
            let source_node: String = row.get(4)?;
            let optional_msg: Option<String> = row.get(5)?;
            let vector_clock_id: i64 = row.get(6)?;

            let mut clock_map = std::collections::HashMap::new();
            let mut vc_stmt = conn.prepare(
                "SELECT site_id, value FROM VectorClockEntry WHERE vector_clock_id = ?1",
            )?;
            let mut rows = vc_stmt.query(params![vector_clock_id])?;
            while let Some(vc_row) = rows.next()? {
                let site_id: String = vc_row.get(0)?;
                let value: i64 = vc_row.get(1)?;
                clock_map.insert(site_id, value);
            }

            Ok(Transaction {
                from_user,
                to_user,
                amount,
                lamport_time,
                source_node,
                optional_msg,
                vector_clock: clock_map,
            })
        }) {
            Ok(tx) => Ok(Some(tx)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(feature = "server")]
pub fn print_users() -> rusqlite::Result<()> {
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare("SELECT unique_name, solde FROM User")?;
        let users = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;

        println!("-- Users --");
        for user in users {
            let (name, solde) = user?;
            println!("{}: {:.2}", name, solde);
        }
        Ok(())
    }
}

#[cfg(feature = "server")]
pub fn get_users() -> rusqlite::Result<Vec<String>> {
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare("SELECT unique_name FROM User")?;
        let users = stmt.query_map([], |row| Ok(row.get::<_, String>(0)?))?;
        let mut users_vec = Vec::new();
        for user in users {
            users_vec.push(user?);
        }
        Ok(users_vec)
    }
}

#[cfg(feature = "server")]
pub fn print_transactions() -> rusqlite::Result<()> {
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT from_user, to_user, amount, lamport_time, source_node, optional_msg, vector_clock_id FROM Transactions",
        )?;
        let txs = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        println!("📜 -- Transactions --");
        println!(
            "┌─────────────────┬─────────────────┬────────────┬────────────┬─────────────────┬──────────────────────┬──────────────────────┐"
        );
        println!(
            "│ {:<15} │ {:<15} │ {:<10} │ {:<10} │ {:<15} │ {:<20} │ {:<20} │",
            "From", "To", "Amount", "Time", "Node", "Message", "Vector Clock"
        );
        println!(
            "├─────────────────┼─────────────────┼────────────┼────────────┼─────────────────┼──────────────────────┼──────────────────────┤"
        );

        for tx in txs {
            let (from, to, amount, time, node, msg, vector_clock_id) = tx?;
            let mut clock_map = std::collections::HashMap::new();
            let mut vc_stmt = conn.prepare(
                "SELECT site_id, value FROM VectorClockEntry WHERE vector_clock_id = ?1",
            )?;
            let mut rows = vc_stmt.query(rusqlite::params![vector_clock_id])?;
            while let Some(vc_row) = rows.next()? {
                let site_id: String = vc_row.get(0)?;
                let value: i64 = vc_row.get(1)?;
                clock_map.insert(site_id, value);
            }

            println!(
                "│ {:<15} │ {:<15} │ {:<10.2} │ {:<10} │ {:<15} │ {:<20} │ {:<20?} │",
                from,
                to,
                amount,
                time,
                node,
                msg.unwrap_or_default(),
                clock_map
            );
        }

        println!(
            "└─────────────────┴─────────────────┴────────────┴────────────┴─────────────────┴──────────────────────┴──────────────────────┘"
        );
        Ok(())
    }
}

#[cfg(feature = "server")]
pub fn print_transaction_for_user(name: &str) -> rusqlite::Result<()> {
    use rusqlite::params;
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT from_user, to_user, amount, lamport_time, source_node, optional_msg, vector_clock_id
        FROM Transactions WHERE from_user = ?1 OR to_user = ?1",
        )?;

        let txs = stmt.query_map(params![name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        println!("📜 -- Transactions for user {} --", name);
        println!(
            "┌─────────────────┬─────────────────┬────────────┬────────────┬─────────────────┬──────────────────────┬──────────────────────┐"
        );
        println!(
            "│ {:<15} │ {:<15} │ {:<10} │ {:<10} │ {:<15} │ {:<20} │ {:<20} │",
            "From", "To", "Amount", "Time", "Node", "Message", "Vector Clock"
        );
        println!(
            "├─────────────────┼─────────────────┼────────────┼────────────┼─────────────────┼──────────────────────┼──────────────────────┤"
        );

        for tx in txs {
            let (from, to, amount, time, node, msg, vector_clock_id) = tx?;
            let mut clock_map = std::collections::HashMap::new();
            let mut vc_stmt = conn.prepare(
                "SELECT site_id, value FROM VectorClockEntry WHERE vector_clock_id = ?1",
            )?;
            let mut rows = vc_stmt.query(rusqlite::params![vector_clock_id])?;
            while let Some(vc_row) = rows.next()? {
                let site_id: String = vc_row.get(0)?;
                let value: i64 = vc_row.get(1)?;
                clock_map.insert(site_id, value);
            }
            println!(
                "│ {:<15} │ {:<15} │ {:<10.2} │ {:<10} │ {:<15} │ {:<20} │ {:<20?} │",
                from,
                to,
                amount,
                time,
                node,
                msg.unwrap_or_default(),
                clock_map
            );
        }

        println!(
            "└─────────────────┴─────────────────┴────────────┴────────────┴─────────────────┴──────────────────────┴──────────────────────┘"
        );
        Ok(())
    }
}

#[cfg(feature = "server")]
pub fn get_transactions_for_user(name: &str) -> rusqlite::Result<Vec<Transaction>> {
    use rusqlite::params;
    {
        let conn = DB_CONN.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT from_user, to_user, amount, lamport_time, source_node, optional_msg, vector_clock_id
        FROM Transactions WHERE from_user = ?1 OR to_user = ?1",
        )?;

        let txs = stmt.query_map(params![name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        let mut txs_vec = Vec::new();
        for tx in txs {
            let (from, to, amount, time, node, msg, vector_clock_id) = tx?;
            let mut clock_map = std::collections::HashMap::new();
            let mut vc_stmt = conn.prepare(
                "SELECT site_id, value FROM VectorClockEntry WHERE vector_clock_id = ?1",
            )?;
            let mut rows = vc_stmt.query(rusqlite::params![vector_clock_id])?;
            while let Some(vc_row) = rows.next()? {
                let site_id: String = vc_row.get(0)?;
                let value: i64 = vc_row.get(1)?;
                clock_map.insert(site_id, value);
            }

            txs_vec.push(Transaction {
                from_user: from,
                to_user: to,
                amount: amount,
                lamport_time: time,
                source_node: node,
                optional_msg: msg,
                vector_clock: clock_map,
            });
        }
        Ok(txs_vec)
    }
}

#[cfg(feature = "server")]
pub fn get_local_transaction_log() -> rusqlite::Result<Vec<Transaction>> {
    let conn = DB_CONN.lock().unwrap();
    let mut stmt = conn.prepare(
    "SELECT from_user, to_user, amount, lamport_time, source_node, optional_msg, vector_clock_id
        FROM Transactions")?;
    let rows = stmt.query_map([], |row| {
        Ok(Transaction {
            from_user: row.get(0)?,
            to_user: row.get(1)?,
            amount: row.get(2)?,
            lamport_time: row.get(3)?,
            source_node: row.get(4)?,
            optional_msg: row.get(5)?,
            vector_clock: std::collections::HashMap::new(),
        })
    })?;

    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}
