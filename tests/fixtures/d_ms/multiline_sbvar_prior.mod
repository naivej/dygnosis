// inventory: d_ms_multiline_sbvar_prior
// A `sbvar` option list and a `prior` option list, both written over several
// lines. Neither head is a command name, so each statement must claim its lines.
var y c k;
varexo e;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
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
sbvar(datafile=msdata,
      freq=4,
      initial_year=1959,
      final_year=2005,
      nlags=2);
alpha.prior(shape=beta,
            mean=3.22,
            variance=0.01);
alpha.options(init=1,
              jscale=0.5,
              bounds=[0, 1]);
