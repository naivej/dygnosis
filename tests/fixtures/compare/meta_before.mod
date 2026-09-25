// compare: symbol metadata added, changed, and removed
var c $c$ (long_name='consumption')
    k (long_name='capital')
    n
    z
    c_hat
    w (long_name='old', long_name='wage');
varexo e;
parameters beta;
beta = 0.99;
heterogeneity_dimension d;
var(heterogeneity=d) h (long_name='hours');
model;
c = beta*c(+1)+k+n+z+c_hat+e;
end;
shocks;
var e; periods 1; values 0.1;
end;
