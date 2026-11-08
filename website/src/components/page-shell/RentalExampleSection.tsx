import { Link } from "@tanstack/react-router"
import { MdxCodeBlock } from "@/components/docs/MdxCodeBlock"

const rentalCode = `#[domain_action(
    id = "rent-bicycle",
    label = "Rent bicycle"
)]
pub trait RentBicycleAction {
    fn rent_bicycle(
        &mut self,
        input: BicycleId,
    ) -> Result<(), BicycleUnavailable>;
}`

export function RentalExampleSection() {
  return (
    <section aria-labelledby="rental-title" className="content-section">
      <h2 id="rental-title" className="section-heading">
        How a rental becomes recorded history
      </h2>
      <p className="section-intro">
        In the bike-rental example, the fleet decides whether a bicycle is
        available. This illustration connects its Rust action declaration to the
        command and events used by the application.
      </p>
      <div className="rental-example">
        <div className="rental-source">
          <a
            href="https://github.com/elbLabs/rostfrei/blob/main/examples/bike-rental/src/domain/bike_rental/rental_fleet/rent_bicycle/action.rs"
            className="source-link"
          >
            rent_bicycle/action.rs
          </a>
          <MdxCodeBlock>
            <code className="language-rust">{rentalCode}</code>
          </MdxCodeBlock>
        </div>
        <ol className="rental-flow" aria-label="Illustrated rental flow">
          <li>
            <span className="text-command">Command</span>
            <h3>
              <code>RentBicycle</code>
            </h3>
            <p>Ask the fleet to rent an available, serviceable bicycle.</p>
          </li>
          <li>
            <span className="text-domain-event">Domain event</span>
            <h3>
              <code>BicycleRented</code>
            </h3>
            <p>Record the accepted rental in the fleet’s private history.</p>
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
        A bicycle that needs maintenance is rejected with{" "}
        <code>BICYCLE_UNAVAILABLE</code>. The command has a recorded outcome
        without adding rental events. Applying the fleet’s event history
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
