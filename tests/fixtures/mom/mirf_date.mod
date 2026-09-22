// inventory: mom_e392_date_in_periods
// A `periods` entry written as a date. The row counts periods from the start of
// the simulation, so it takes integers and integer ranges.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo e; periods 2000Q1; values 1;
end;
