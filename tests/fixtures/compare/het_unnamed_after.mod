// compare: an unnamed row may text-match a named row
var y;
heterogeneity_dimension h;
var(heterogeneity=h) a;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='euler'] a = 1;
end;
