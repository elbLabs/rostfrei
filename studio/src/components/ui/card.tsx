import * as React from "react";
import { cn } from "../../lib/utils";

function Card({ className, ...props }: React.ComponentProps<"section">) {
  return <section className={cn("ui-card", className)} {...props} />;
}

function CardHeader({ className, ...props }: React.ComponentProps<"header">) {
  return <header className={cn("ui-card__header", className)} {...props} />;
}

function CardTitle({ className, ...props }: React.ComponentProps<"h2">) {
  return <h2 className={cn("ui-card__title", className)} {...props} />;
}

function CardDescription({ className, ...props }: React.ComponentProps<"p">) {
  return <p className={cn("ui-card__description", className)} {...props} />;
}

function CardContent({ className, ...props }: React.ComponentProps<"div">) {
  return <div className={cn("ui-card__content", className)} {...props} />;
}

function CardFooter({ className, ...props }: React.ComponentProps<"footer">) {
  return <footer className={cn("ui-card__footer", className)} {...props} />;
}

export { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle };
