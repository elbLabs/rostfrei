import { Link } from "@tanstack/react-router"
import { MdxCodeBlock } from "@/components/docs/MdxCodeBlock"

const rentalCode = `#[derive(Command, Clone, Debug)]
#[domain(
    id = "collect-rental-payment",
    label = "Collect rental payment",
    context = Finance
)]
pub struct CollectRentalPayment {
    pub payment_id: PaymentId,
}`

export function RentalExampleSection() {
  return (
    <section aria-labelledby="rental-title" className="content-section">
      <h2 id="rental-title" className="section-heading">
        How a rental becomes recorded history
      </h2>
      <p className="section-intro">
        In the bike-rental example, customers reserve a category and staff selects
        the bicycle at collection. Payment approval starts the rental. This
        illustration connects the Rust command to its recorded effects.
      </p>
      <div className="rental-example">
        <div className="rental-source">
          <a
            href="https://github.com/elbLabs/rostfrei/blob/main/examples/bike-rental/src/domain/finance/rental_payment/process_payment/command.rs"
            className="source-link"
          >
            process_payment/command.rs
          </a>
          <MdxCodeBlock>
            <code className="language-rust">{rentalCode}</code>
          </MdxCodeBlock>
        </div>
        <ol className="rental-flow" aria-label="Illustrated rental flow">
          <li>
            <span className="text-command">Command</span>
            <h3>
              <code>CollectRentalPayment</code>
            </h3>
            <p>Collect the agreed rent and deposit for the pending collection.</p>
          </li>
          <li>
            <span className="text-domain-event">Domain event</span>
            <h3>
              <code>BicycleRented</code>
            </h3>
            <p>Rental records collection after Finance publishes its approval.</p>
          </li>
          <li>
            <span className="text-integration-event">Integration event</span>
            <h3>
              <code>BicycleRentalStarted</code>
            </h3>
            <p>Notify other parts of the application after the commit.</p>
          </li>
        </ol>
      </div>
      <p className="section-intro">
        A declined payment records <code>RentalPaymentDeclined</code> and releases
        the bicycle while keeping the booking. Applying the fleet’s event history
        reconstructs its state without running those commands again.
      </p>
      <Link
        to="/docs"
        hash="why-events-are-the-foundation"
        className="section-link text-link"
      >
        Read about event sourcing and its tradeoffs →
      </Link>
    </section>
  )
}
