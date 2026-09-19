// inventory: d_ms_multiline_switching
// The manual's `markov_switching` example shape, written over five lines. 7.1
// accepts it; the `parameters=[…]` option value must never read as a declaration.
var y c k;
varexo e;
parameters alpha delta theta;
alpha = 0.36;
delta = 0.025;
theta = 0.5;
model;
c*theta = (1-alpha)*y;
y = k(-1)^alpha;
k = (1-delta)*k(-1) + y - c + e;
end;
initval;
y = 1;
c = 0.5;
k = 1;
end;
shocks;
var e; stderr 0.1;
end;
markov_switching(chain=1,
                 number_of_regimes=2,
                 duration=2.5,
                 parameters=[alpha, delta, theta],
                 number_of_lags=1);
