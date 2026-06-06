big plan: extract and unify and build a centralize design-system cratte for all common base components and utilities, and then build the project on top of it. This will allow us to have a consistent design language across all our projects and also make it easier to maintain and update our components in the future.

--

check @ file modal

when select a first level file, the file isn't selected, but treated as folder (expanded), and the file is selected only when click the file name, which is not intuitive. we should select the file when click anywhere on the file item, and expand the folder when click the expand icon.

when the top level file is selected, the file should be selected in the TUI and dismiss the file modal, but currently it just expand the folder and doesn't select the file.
