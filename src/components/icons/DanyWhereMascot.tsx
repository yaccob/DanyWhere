import DanyWherePin from "./DanyWherePin";

/** Onboarding's waving figure: DanyWhere's pin in the accent colour. */
const DanyWhereMascot = ({ className = "" }: { className?: string }) => (
  <DanyWherePin className={`${className} text-logo-primary`} />
);

export default DanyWhereMascot;
