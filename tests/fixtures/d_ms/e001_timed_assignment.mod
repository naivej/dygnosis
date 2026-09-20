// inventory: e001_timed_assignment
// `y(1) = 2;` — the top-level assignment takes a plain symbol; 7.1 refuses `unexpected '(', expecting EQUAL or '.'`.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
y(1) = 2;
