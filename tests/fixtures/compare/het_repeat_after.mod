// compare: repeated name pairs only on exact text and tags
var y;
heterogeneity_dimension h;
var(heterogeneity=h) a;
model;
[name='anchor'] y = 0;
end;
model(heterogeneity=h);
[name='policy', bind='ELB'] a = 2;
[name='policy', relax='ELB'] a = 1;
end;
