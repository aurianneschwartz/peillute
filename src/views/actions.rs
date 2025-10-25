//! Transaction action components for the Peillute application
//!
//! This module provides components for various financial transactions in the system,
//! including viewing transaction history, making deposits, withdrawals, payments,
//! refunds, and transfers between users.

use dioxus::prelude::*;

// show all transactions as vertical card list
/// Transaction history component
///
/// Displays a list of all transactions for a specific user, showing details such as
/// the source and destination users, amount, and any associated messages.
#[component]
pub fn History(name: String) -> Element {
    let name = std::rc::Rc::new(name);
    let name_for_future = name.clone();

    let transactions_resource = use_resource(move || {
        let name_clone = name_for_future.clone();
        async move { get_transactions_for_user_server(name_clone.to_string()).await }
    });

    rsx! {
        div { id: "history-page",
            match &*transactions_resource.read() {
                None => rsx! {
                    p { "Loading history..." }
                },
                Some(Ok(transactions)) => {
                    if transactions.is_empty() {
                        rsx! {
                            p { "No transactions found for {name}." }
                        }
                    } else {
                        rsx! {
                            ul { class: "transactions-list",
                                for transaction in transactions.iter() {
                                    li {
                                        key: "{transaction.lamport_time}-{transaction.source_node}",
                                        class: "transaction-card",
                                        p {
                                            strong { "From:" }
                                            " {transaction.from_user}"
                                        }
                                        p {
                                            strong { "To:" }
                                            " {transaction.to_user}"
                                        }
                                        p {
                                            strong { "Amount:" }
                                            " {transaction.amount:.2}"
                                        }
                                        if let Some(msg) = &transaction.optional_msg {
                                            if !msg.is_empty() {
                                                p {
                                                    strong { "Message:" }
                                                    " {msg}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    p { class: "error-message", "Error loading history: {e}" }
                },
            }
        }
    }
}

// take the username and collect the an amount (float from form) to make a withdrawal
/// Withdrawal component
///
/// Provides a form for users to withdraw money from their account, with input
/// validation to ensure positive amounts and sufficient funds.
#[component]
pub fn Withdraw(name: String) -> Element {
    let mut withdraw_amount = use_signal(|| 0f64);
    let name = std::rc::Rc::new(name);

    let mut error_signal = use_signal(|| None::<String>);

    let name_for_future = name.clone();

    rsx! {
        div { id: "withdraw-form",
            form {
                label { r#for: "fwithdraw", "Withdraw amount :" }
                input {
                    r#type: "number",
                    id: "form-withdraw",
                    r#name: "fwithdraw",
                    step: 0.01,
                    value: "{withdraw_amount}",
                    oninput: move |event| {
                        if let Ok(as_number) = event.value().parse::<f64>() {
                            withdraw_amount.set(as_number);
                        }
                    },
                }
                button {
                    r#type: "button",
                    onclick: move |_| {
                        let name = name_for_future.clone();
                        let amount = *withdraw_amount.read();
                        async move {
                            if amount >= 0.0 {
                                if let Ok(_) = withdraw_for_user_server(name.to_string(), amount).await {
                                    withdraw_amount.set(0.0);
                                    error_signal.set(None);
                                }
                            } else {
                                error_signal
                                    .set(
                                        Some(
                                            format!("Please enter a positive amount, you gave {amount}."),
                                        ),
                                    );
                            }
                        }
                    },
                    "Submit"
                }
            }
            if let Some(error) = &*error_signal.read() {
                p { class: "error-message", "{error}" }
            }
        }
    }
}

const COCA_IMG: Asset = asset!("/assets/images/coca.png");
const CHIPS_IMG: Asset = asset!("/assets/images/chips.png");
const SANDWICH_IMG: Asset = asset!("/assets/images/sandwich.png");
const COFFEE_IMG: Asset = asset!("/assets/images/coffee.png");

const PRODUCTS: &[(&str, f64, Asset)] = &[
    ("Coca", 1.50, COCA_IMG),
    ("Chips", 2.00, CHIPS_IMG),
    ("Sandwich", 4.50, SANDWICH_IMG),
    ("Coffee", 1.20, COFFEE_IMG),
];

// take the username and collect the an amount (float from form) to make a payment
/// Payment component
///
/// Implements a product catalog interface where users can select items to purchase,
/// with a running total and order summary. Supports multiple products with
/// individual quantity selection.
#[component]
pub fn Pay(name: String) -> Element {
    let mut product_quantities = use_signal(|| vec![0u32; PRODUCTS.len()]);
    let name_for_payment = std::rc::Rc::new(name.clone());

    let mut error_signal = use_signal(|| None::<String>);

    let handle_pay = move |_| {
        let current_quantities = product_quantities.read().clone();
        let name_clone = name_for_payment.clone();

        let mut total_amount = 0.0;
        for (i, &(_, price, _)) in PRODUCTS.iter().enumerate() {
            if let Some(&quantity) = current_quantities.get(i) {
                total_amount += price * quantity as f64;
            }
        }

        spawn(async move {
            if total_amount > 0.0 {
                if let Ok(_) = pay_for_user_server(name_clone.to_string(), total_amount).await {
                    tracing::info!(
                        user = %name_clone,
                        amount = total_amount,
                        operation = "payment",
                        "Payment successful.");
                    product_quantities.set(vec![0u32; PRODUCTS.len()]);
                    error_signal.set(None);
                }
            } else {
                tracing::warn!(
                    user = %name_clone,
                    amount = total_amount,
                    operation = "payment",
                    "Attempted to pay with a total of 0.0. No action taken.");
                error_signal.set(Some(
                    "Cannot pay €0. Please select at least one item.".to_string(),
                ));
            }
        });
    };

    let current_total_display = use_memo(move || {
        let mut total = 0.0;
        let quantities_read = product_quantities.read();
        for (i, &(_, price, _)) in PRODUCTS.iter().enumerate() {
            if let Some(&quantity) = quantities_read.get(i) {
                total += price * quantity as f64;
            }
        }
        total
    });

    rsx! {
        div { id: "pay-page",
            div {
                for (index , (product_name , price , image_path)) in PRODUCTS.iter().enumerate() {
                    div { key: "{product_name}-{index}",
                        img { src: "{image_path}", alt: "{product_name}" }
                        div { class: "product-info",
                            h3 { "{product_name}" }
                            p { "€{price:.2}" }
                            div {
                                label { r#for: "qty-{index}", "Quantity:" }
                                input {
                                    r#type: "number",
                                    id: "qty-{index}",
                                    min: "0",
                                    value: "{product_quantities.read()[index]}",
                                    oninput: move |event| {
                                        let mut pq_signal_for_input = product_quantities;
                                        if let Ok(new_quantity) = event.value().parse::<u32>() {
                                            let mut quantities_writer = pq_signal_for_input.write();
                                            if index < quantities_writer.len() {
                                                quantities_writer[index] = new_quantity;
                                            }
                                        } else if event.value().is_empty() {
                                            let mut quantities_writer = pq_signal_for_input.write();
                                            if index < quantities_writer.len() {
                                                quantities_writer[index] = 0;
                                            }
                                        }
                                    },
                                }
                            }
                        }
                    }
                }
            }

            div { class: "cart-summary",
                h2 { "Order Summary" }
                h3 { "Total: €{current_total_display():.2}" }
                form {
                    button {
                        r#type: "button",
                        disabled: current_total_display() == 0.0,
                        onclick: handle_pay,
                        "Pay Now"
                    }
                }
            }

            if let Some(error) = &*error_signal.read() {
                p { class: "error-message", "{error}" }
            }
        }
    }
}

// show all transactions as vertical card list
// allow the user to select a transaction to refund it
/// Refund component
///
/// Displays a list of transactions that can be refunded, allowing users to
/// reverse previous transactions. Shows transaction details and provides
/// refund functionality.
#[component]
pub fn Refund(name: String) -> Element {
    let name = std::rc::Rc::new(name);
    let name_for_future = name.clone();

    let mut error_signal = use_signal(|| None::<String>);

    let transactions_resource = use_resource(move || {
        let name_clone = name_for_future.clone();
        async move { get_transactions_for_user_server(name_clone.to_string()).await }
    });

    rsx! {
        div { id: "refund-page",
            match &*transactions_resource.read() {
                None => rsx! {
                    p { "Loading history..." }
                },
                Some(Ok(transactions)) => {
                    let name_clone = name.clone();
                    if transactions.is_empty() {
                        rsx! {
                            p { "No transactions found for {name_clone}." }
                        }
                    } else {
                        rsx! {
                            ul { class: "transactions-list",
                                for transaction in transactions.iter() {
                                    li {
                                        key: "{transaction.lamport_time}-{transaction.source_node}",
                                        class: "transaction-card",
                                        p {
                                            strong { "From:" }
                                            " {transaction.from_user}"
                                        }
                                        p {
                                            strong { "To:" }
                                            " {transaction.to_user}"
                                        }
                                        p {
                                            strong { "Amount:" }
                                            " {transaction.amount:.2}"
                                        }
                                        if let Some(msg) = &transaction.optional_msg {
                                            if !msg.is_empty() {
                                                p {
                                                    strong { "Message:" }
                                                    " {msg}"
                                                }
                                            }
                                        }
                                        {
                                            let transaction_for_refund = transaction.clone();
                                            let name_for_refund = name.clone();
                                            let mut resource_to_refresh = transactions_resource.clone();
                                            rsx! {
                                                button {
                                                    r#type: "button",
                                                    onclick: move |_| {
                                                        let name_for_future = name_for_refund.clone();
                                                        let transaction_for_future = transaction_for_refund.clone();
                                                        async move {
                                                            if let Ok(_) = refund_transaction_server(
                                                                    name_for_future.to_string(),
                                                                    transaction_for_future.lamport_time,
                                                                    transaction_for_future.source_node,
                                                                )
                                                                .await
                                                            {
                                                                if let Ok(_) = get_transactions_for_user_server(
                                                                        name_for_future.to_string(),
                                                                    )
                                                                    .await
                                                                {
                                                                    error_signal.set(None);
                                                                    resource_to_refresh.restart();
                                                                }
                                                            }
                                                        }
                                                    },
                                                    "Refund"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if let Some(error) = &*error_signal.read() {
                                p { class: "error-message", "{error}" }
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    p { class: "error-message", "Error loading transactions: {e}" }
                },
            }
        }
    }
}

// allow to select a user between all users (except the current one)
// and allow user to transfer money with an amout (float from form) to another user
// allow user to add a message to the transaction
/// Transfer component
///
/// Enables users to transfer money to other users in the system, with features for:
/// - Selecting the recipient from a list of available users
/// - Specifying the transfer amount
/// - Adding an optional message to the transaction
/// - Generating random messages for fun
#[component]
pub fn Transfer(name: String) -> Element {
    let mut transfer_amount = use_signal(|| 0f64);
    let mut transfer_message = use_signal(String::new);
    let mut selected_user = use_signal(String::new);
    let name = std::rc::Rc::new(name);
    let name_for_future = name.clone();

    let mut error_signal = use_signal(|| None::<String>);

    let users_resource = use_resource({
        move || {
            let current_user = name_for_future.clone();
            async move {
                let all_users = get_users_server().await.unwrap_or_default();
                all_users
                    .into_iter()
                    .filter(|u| u != current_user.as_ref())
                    .collect::<Vec<_>>()
            }
        }
    });

    rsx! {
        div { id: "transfer-page",
            match &*users_resource.read() {
                None => rsx! {
                    p { "Loading users..." }
                },
                Some(users) => rsx! {
                    form {
                        label { r#for: "user-select", "Select user to transfer to:" }
                        select {
                            id: "user-select",
                            onchange: move |evt| {
                                selected_user.set(evt.value());
                            },
                            option {
                                value: "",
                                disabled: true,
                                selected: selected_user.read().is_empty(),
                                "Choose a user"
                            }
                            for user in users {
                                option { key: "{user}", value: "{user}", "{user}" }
                            }
                        }
                        label { r#for: "transfer-amount", "Amount to transfer:" }
                        input {
                            r#type: "number",
                            id: "transfer-amount",
                            step: 0.01,
                            value: "{transfer_amount}",
                            oninput: move |evt| {
                                if let Ok(val) = evt.value().parse::<f64>() {
                                    transfer_amount.set(val);
                                }
                            },
                        }
                        label { r#for: "transfer-message", "Message (optional):" }
                        input {
                            r#type: "text",
                            id: "transfer-message",
                            value: "{transfer_message}",
                            oninput: move |evt| {
                                transfer_message.set(evt.value());
                            },
                        }
                        button {
                            r#type: "button",
                            onclick: move |_| {
                                async move {
                                    if let Ok(message) = get_random_message_server().await {
                                        transfer_message.set(message);
                                    }
                                }
                            },
                            "Select a random message"
                        }
                        button {
                            r#type: "button",
                            onclick: move |_| {
                                let to_user = selected_user.read().clone();
                                let amount = *transfer_amount.read();
                                let message = transfer_message.read().clone();
                                let from_user = name.clone();
                                async move {
                                    if !to_user.is_empty() && amount > 0.0 {
                                        if let Ok(_) = transfer_from_user_to_user_server(
                                                from_user.to_string(),
                                                to_user,
                                                amount,
                                                message,
                                            )
                                            .await
                                        {
                                            transfer_amount.set(0.0);
                                            transfer_message.set(String::new());
                                            selected_user.set(String::new());
                                            error_signal.set(None);
                                        }
                                    } else {
                                        error_signal
                                            .set(
                                                Some(
                                                    "Please select a user and enter a positive amount."
                                                        .to_string(),
                                                ),
                                            );
                                    }
                                }
                            },
                            "Transfer"
                        }
                    }
                },
            }
            if let Some(error) = &*error_signal.read() {
                p { class: "error-message", "{error}" }
            }
        }
    }
}

// take the username and collect the an amount (float from form) to make a deposit
/// Deposit component
///
/// Provides a form for users to deposit money into their account, with input
/// validation to ensure positive amounts.
#[component]
pub fn Deposit(name: String) -> Element {
    let mut deposit_amount = use_signal(|| 0f64);
    let name = std::rc::Rc::new(name);

    let mut error_signal = use_signal(|| None::<String>);

    let name_for_future = name.clone();

    rsx! {
        div { id: "deposit-form",
            form {
                label { r#for: "fdeposit", "Deposit amount :" }
                input {
                    r#type: "number",
                    id: "form-deposit",
                    r#name: "fdeposit",
                    step: 0.01,
                    value: "{deposit_amount}",
                    oninput: move |event| {
                        if let Ok(as_number) = event.value().parse::<f64>() {
                            deposit_amount.set(as_number);
                        }
                    },
                }
                button {
                    r#type: "button",
                    onclick: move |_| {
                        let name = name_for_future.clone();
                        let amount = *deposit_amount.read();
                        async move {
                            if amount >= 0.0 {
                                if let Ok(_) = deposit_for_user_server(name.to_string(), amount).await {
                                    deposit_amount.set(0.0);
                                    error_signal.set(None);
                                }
                            } else {
                                error_signal
                                    .set(
                                        Some(
                                            format!("Please enter a positive amount, you gave {amount}."),
                                        ),
                                    );
                            }
                        }
                    },
                    "Submit"
                }
            }
            if let Some(error) = &*error_signal.read() {
                p { class: "error-message", "{error}" }
            }
        }
    }
}

#[cfg(feature = "server")]
const RANDOM_MESSAGE: &[&str] = &[
    "Prend tes 200 balles et va te payer des cours de theatre",
    "C'est pour toi bb",
    "Love sur toi",
    "Phrase non aléatoire",
    "Votre argent messire",
    "Acompte sur livraison cocaine",
    "Votre argent seigneur",
    "Pour tout ce que tu fais pour moi",
    "Remboursement horny.com",
    "Puta, où tu étais quand j'mettais des sept euros d'essence",
    "Parce que l'argent n'est pas un problème pour moi",
    "Tiens le rat",
    "Pour le rein",
    "Abonnement OnlyFans",
    "Pour notre dernière nuit, pourboire non compris",
    "ça fait beaucoup la non ?",
    "Chantage SexTape",
    "Argent sale",
    "Adhésion front national",
    "Ce que tu sais...",
    "Remboursement dot de ta soeur",
    "Rien à ajouter",
    "Téléphone rose",
    "Raison : \"GnaGnaGna moi je paye pas pour vous\"",
    "Fond de tiroir",
    "Epilation des zones intimes",
    "Pour m'avoir gratouillé le dos",
    "La reine Babeth vous offre cet argent",
    "Nan t'inquiete",
];

#[cfg(feature = "server")]
fn get_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[cfg(feature = "server")]
fn lcg(seed: u64) -> u64 {
    const A: u64 = 6364136223846793005;
    const C: u64 = 1;
    seed.wrapping_mul(A).wrapping_add(C)
}

#[server]
async fn get_random_message_server() -> Result<String, ServerFnError> {
    let seed = get_seed();
    let random_number = lcg(seed);
    let message = RANDOM_MESSAGE[random_number as usize % RANDOM_MESSAGE.len()];
    Ok(message.to_string())
}

#[server]
async fn get_users_server() -> Result<Vec<String>, ServerFnError> {
    use crate::db;
    let users = db::get_users()?;
    Ok(users)
}

#[server]
async fn deposit_for_user_server(user: String, amount: f64) -> Result<(), ServerFnError> {
    if amount < 0.0 {
        return Err(ServerFnError::new("Amount cannot be negative."));
    }

    if let Err(e) = crate::control::enqueue_critical(crate::control::CriticalCommands::Deposit {
        name: user,
        amount: amount,
    })
    .await
    {
        return Err(ServerFnError::new(format!(
            "[SERVER] Failed to diffuse deposit : {e}"
        )));
    }

    Ok(())
}

#[server]
async fn withdraw_for_user_server(user: String, amount: f64) -> Result<(), ServerFnError> {
    if amount < 0.0 {
        return Err(ServerFnError::new("Amount cannot be negative."));
    }

    if let Err(e) = crate::control::enqueue_critical(crate::control::CriticalCommands::Withdraw {
        name: user,
        amount: amount,
    })
    .await
    {
        return Err(ServerFnError::new(format!(
            "[SERVER] Failed to withdraw : {e}"
        )));
    }

    Ok(())
}

#[server]
async fn pay_for_user_server(user: String, amount: f64) -> Result<(), ServerFnError> {
    if amount < 0.0 {
        return Err(ServerFnError::new("Amount cannot be negative."));
    }

    if let Err(e) = crate::control::enqueue_critical(crate::control::CriticalCommands::Pay {
        name: user,
        amount: amount,
    })
    .await
    {
        return Err(ServerFnError::new(format!("[SERVER] Failed to pay : {e}")));
    }

    Ok(())
}

#[server]
async fn transfer_from_user_to_user_server(
    from_user: String,
    to_user: String,
    amount: f64,
    _optional_message: String,
) -> Result<(), ServerFnError> {
    if amount < 0.0 {
        return Err(ServerFnError::new("Amount cannot be negative."));
    }

    if let Err(e) = crate::control::enqueue_critical(crate::control::CriticalCommands::Transfer {
        from: from_user,
        to: to_user,
        amount: amount,
    })
    .await
    {
        return Err(ServerFnError::new(format!(
            "[SERVER] Failed to make the transfer: {e}"
        )));
    }

    Ok(())
}

#[server]
async fn get_transactions_for_user_server(
    name: String,
) -> Result<Vec<crate::db::Transaction>, ServerFnError> {
    if let Ok(data) = crate::db::get_transactions_for_user(&name) {
        Ok(data)
    } else {
        Err(ServerFnError::new("User not found."))
    }
}

#[server]
async fn refund_transaction_server(
    name: String,
    lamport_time: i64,
    transac_node: String,
) -> Result<(), ServerFnError> {
    if let Err(e) = crate::control::enqueue_critical(crate::control::CriticalCommands::Refund {
        name: name,
        lamport: lamport_time,
        node: transac_node,
    })
    .await
    {
        return Err(ServerFnError::new(format!(
            "[SERVER] Failed to refund : {e}"
        )));
    }

    Ok(())
}
